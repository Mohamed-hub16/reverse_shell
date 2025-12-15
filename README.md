# Rust Secure Reverse Shell & C2

> **Avertissement** : Ce projet a été développé dans un cadre **éducatif et académique** pour apprendre le langage Rust, la programmation réseau et les concepts de sécurité. L'utilisation de ce code pour attaquer des cibles sans autorisation préalable est illégale.

## Présentation

Ce projet implémente un **Reverse Shell** complet écrit en **Rust**, accompagné de son **Serveur C2 (Command & Control)**.

L'objectif principal était de créer un canal de communication furtif et sécurisé entre une machine victime (Client) et une machine attaquante (Serveur). Contrairement aux reverse shells basiques (type Netcat), celui-ci utilise une couche de chiffrement **TLS** pour empêcher l'analyse du trafic par des IDS/IPS.

### Fonctionnalités

* **Communication Chiffrée** : Tout le trafic est encapsulé dans un tunnel TLS 1.2/1.3 (via `native-tls`).
* **Architecture Client/Serveur** :
    * **Client (`reverse_shell`)** : Compatible Windows & Linux. Tente de se reconnecter automatiquement.
    * **Serveur (`server_c2`)** : Gère les connexions entrantes, maintient la session et sauvegarde les fichiers exfiltrés.
* **Commandes Internes** :
    * `download <fichier>` : Récupère un fichier distant et le sauvegarde proprement sur le serveur.
    * `upload <texte> <nom>` : Crée un fichier sur la cible avec le contenu spécifié.
    * `cd <dossier>` : Navigation persistante dans les répertoires.
* **Commandes Système** : Exécution de toutes les commandes natives de l'OS (`dir`, `ipconfig`, `whoami`, etc.).

---

## Architecture Technique

```mermaid
sequenceDiagram
    participant Victim as Client (Windows/Rust)
    participant Attacker as Server C2 (Linux/Rust)
    
    Note over Victim, Attacker: Phase de Connexion
    Victim->>Attacker: TCP Connect (Port 4444)
    Attacker->>Victim: Server Hello (Certificat TLS)
    Victim->>Attacker: Handshake TLS & Vérification
    Note over Victim, Attacker: Canal Sécurisé (Chiffré)
    
    loop Session Shell
        Attacker->>Victim: Commande (ex: "whoami")
        Victim->>Victim: Exécution locale
        Victim-->>Attacker: Résultat chiffré
    end
