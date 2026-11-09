# FlyffRewriteRust

Réécriture de Flyff V15 en **Rust**, avec un serveur de jeu, un serveur HTTP et un client jouable dans le navigateur. Le client utilise **TypeScript, React, three.js et WebAssembly** ; les règles et la simulation partagées sont implémentées en Rust.

Le projet vise une distribution Linux sous la forme d'un exécutable `flyff` embarquant le client web. **Les ressources originales ne sont pas incluses. Il faut se munir séparément d'une version V15 de Flyff et fournir son dossier `Resource/`.**

**Projet en développement.** Ce dépôt contient une implémentation étendue, mais ne constitue pas une version finale ni une garantie de parité complète avec le jeu original.

## Aperçu en jeu

Captures du projet avec les ressources V15 chargées.

**Flarine : personnages, monstres et effets de compétences.**

![Personnages et Aibatts près de Flarine, avec des effets de compétences et l'interface de jeu](docs/images/flarine-gameplay.png)

**Flarine : environnement, PNJ et inventaire du personnage.**

![Vue de Flarine avec les PNJ, le personnage et son inventaire ouvert](docs/images/flarine-inventory.png)

## Avancement et travail restant

État établi à partir des sources le **9 novembre 2026**. Une case cochée dans « Implémenté » indique que le code du système est présent ; elle ne garantit ni sa validation complète en jeu ni une parité totale avec Flyff V15. Cette liste synthétise l'état du dépôt et ne remplace pas les documents de conception historiques.

### Implémenté dans les sources

- [x] Serveur Rust, HTTP, sessions et transport WebSocket / Protocol Buffers.
- [x] Comptes, connexion, création et sélection des personnages.
- [x] Chargement et conversion des ressources V15, client web 3D et interface de jeu.
- [x] Simulation partagée en Rust / WASM : déplacements au sol et en vol, collisions et règles de jeu.
- [x] Monstres, IA, combat, compétences, buffs et statistiques.
- [x] Inventaire, équipements, échanges, boutiques, banque, courrier et améliorations d'objets.
- [x] Chat, groupes, messagerie, guildes, couples et mentorat.
- [x] Dialogues Lua 5.3, scripts PNJ et moteur de quêtes avec objectifs et récompenses.
- [x] Systèmes de familiers, logements, instances, sièges, Lord, taxes, Secret Room, Rainbow Race et quiz.
- [x] Persistance SQLite, migrations et outils de sauvegarde.
- [x] Console d'administration, gestion des joueurs et journal d'audit, avec les limites indiquées ci-dessous.
- [x] Outillage de développement, génération, compilation web et préparation de la distribution Linux.

### Fonctions à terminer ou à arbitrer

- [ ] Implémenter la commande `replay`, la commande `grant` en ligne de commande et les exports encore indisponibles ; l'export `gamedata-json` existe déjà.
- [ ] Activer le pilotage du mode maintenance depuis l'administration.
- [ ] Terminer les neuf panneaux d'administration du contenu : événements, Lord, taxes, Secret Room, sièges, Rainbow Race, quiz, instances et logements. Leurs routes répondent encore « fonctionnalité indisponible », même lorsque le système de jeu correspondant existe.
- [ ] Traiter les 15 commandes GM marquées différées, notamment météo, musique/sons, listes de joueurs et maintenance, ou documenter leur exclusion définitive.
- [ ] Finaliser les helpers des quêtes de boss de guilde/groupe, dont Wormon, actuellement différés.
- [ ] Vérifier et compléter le contenu des quêtes à partir des ressources V15 fournies séparément. Le petit catalogue synthétique du dépôt ne constitue pas une campagne V15 complète.
- [ ] Implémenter le TLS natif si cette option est retenue ; actuellement, HTTPS nécessite une terminaison TLS par reverse proxy.
- [ ] Clarifier le périmètre des options réservées, notamment le second mot de passe, et de la compression WebSocket non prise en charge.
- [ ] Actualiser les références aux anciens tests et documents absents, et revoir les contenus synthétiques conservés pour ne garder que les compléments utiles au jeu.

Les points ci-dessus sont notamment vérifiables dans la [CLI](crates/flyff-server/src/cli.rs), le [raccordement de la commande grant](crates/flyff-server/src/wiring.rs), le [mode maintenance](crates/flyff-server/src/admin/api/server/maintenance.rs), les [panneaux de contenu](crates/flyff-server/src/admin/api/content.rs), le [catalogue GM](crates/flyff-game/src/gm/table.rs) et les [quêtes Wormon](crates/flyff-game/src/quest/wormon.rs).

### Validation avant une distribution stable

Ces cases restent ouvertes tant qu'une recette complète et reproductible n'est pas documentée.

- [ ] Construire et vérifier toute la chaîne : module WASM, bundle web et distribution Linux avec client embarqué.
- [ ] Vérifier une installation vierge avec un dossier `Resource/` V15 externe et une configuration générique.
- [ ] Valider les parcours complets : création de personnage, combat, progression, inventaire, échanges, quêtes et reconnexion.
- [ ] Vérifier les sauvegardes, restaurations et migrations avec des données représentatives.
- [ ] Évaluer la stabilité sur la durée, les performances avec plusieurs joueurs et la compatibilité des navigateurs.
- [ ] Comparer les comportements et le rendu au jeu V15 pour relever et corriger les écarts.

Les vérifications déjà effectuées lors de la préparation du dépôt couvrent la compilation de contrôle Rust, le formatage, TypeScript et un lint ciblé. La vérification TypeScript utilisait les déclarations de l'API WASM, sans reconstruire le module : elle ne clôt donc pas la validation de la chaîne complète.

### Préparation du dépôt public

- [x] Publier les sources sur GitHub avec un README d'installation et de compilation.
- [x] Exclure les ressources originales et préciser qu'une V15 doit être fournie séparément.
- [x] Retirer les suites de tests, configurations privées et données d'exploitation de la publication.
- [x] Ajouter les captures d'écran du jeu au README.

Les ressources originales, les suites de tests publiques et les configurations privées restent volontairement hors du périmètre de cette distribution. Les validations restantes n'impliquent pas de les ajouter au dépôt.

## Prérequis

| Composant | Version / usage |
| --- | --- |
| Rust via rustup | **1.99.0**, fixé dans `rust-toolchain.toml` |
| Node.js | **24.x** ; `.node-version` indique la version de référence |
| pnpm | **12.11.2**, fixé dans `web/package.json` |
| Ressources Flyff V15 | Une version V15 fournie séparément, avec son dossier `Resource/` |
| Compilateur C et outils de compilation | SQLite, Lua et certaines dépendances natives |
| Cible Rust `wasm32-unknown-unknown` | Module WebAssembly du client |
| Outils de génération | Versions fixées dans `xtask/src/tools.rs` ; `cargo xtask tools check` les vérifie |
| Linux x86-64 + `musl-gcc` | Construction de la distribution statique Linux |

Le pipeline de distribution cible Linux x86-64. Pour construire ce paquet depuis macOS ou Windows, utiliser un environnement Linux adapté.

## Installation et démarrage

```bash
git clone https://github.com/Laarm/FlyffRewriteRust.git
cd FlyffRewriteRust

# Installe la toolchain et les composants fixés par le dépôt.
rustup show

# Avec Corepack installé : active pnpm à la version du package.json.
corepack enable
pnpm --dir web install --frozen-lockfile

# Installe les outils auxiliaires, puis vérifie l'environnement.
cargo xtask tools install
cargo xtask doctor
```

Certaines dépendances système doivent être installées séparément. Sur Debian/Ubuntu, `cargo xtask tools install --apt` peut prendre en charge les paquets système prévus par l'outil.

### Ressources du jeu

**Munissez-vous d'une version V15 de Flyff** : le dossier original `Resource/` est nécessaire pour démarrer un monde. Le dépôt ne fournit ni client original, ni archive V15, ni ressources originales. Les ressources d'une autre version ne sont pas garanties compatibles.

Conservez votre dossier `Resource/` en dehors du dépôt et configurez son emplacement avant de lancer le serveur :

```bash
export FLYFF_RESOURCE="/path/to/Flyff V15/Resource"
```

`FLYFF_SOURCE` est facultatif pour jouer : il désigne les anciennes sources C++ et sert à certains générateurs. Les sources générées nécessaires à la compilation sont déjà présentes.

```bash
# Facultatif : uniquement pour les outils qui en ont besoin.
export FLYFF_SOURCE="/path/to/Flyff V15/Source"
```

### Serveur et client en développement

```bash
cargo xtask dev --resource "$FLYFF_RESOURCE"
```

Cette commande prépare le module WASM, lance le serveur sur `127.0.0.1:8080`, le client Vite sur **http://localhost:5173/** et la recompilation du module WASM. L'état local est placé dans `.dev/`.

Pour lancer uniquement le serveur :

```bash
cargo run --locked -p flyff-server -- serve \
  --home .dev \
  --resource "$FLYFF_RESOURCE" \
  --bind 127.0.0.1:8080
```

Le lancement du serveur seul ne construit pas le client web. Pour servir un client déjà construit, ajouter `--web-dir web/dist`.

### Configuration et administration

Le serveur utilise `flyff.toml` dans son dossier d'exécution. Le modèle commenté se trouve dans [`crates/flyff-server/assets/flyff.toml.example`](crates/flyff-server/assets/flyff.toml.example).

```bash
cargo run --locked -p flyff-server -- --help
cargo run --locked -p flyff-server -- config default
```

Les variables `FLYFF__SECTION__KEY` permettent de remplacer des paramètres, par exemple `FLYFF__NET__BIND=127.0.0.1:8080`. La console d'administration est disponible à `/admin` lorsque le client web est servi ; son accès dépend de la configuration et de l'authentification du serveur.

Conserver les configurations d'exploitation, comptes, sauvegardes, jetons d'administration et certificats hors du dépôt. Le `.gitignore` couvre les emplacements usuels.

## Compilation

```bash
# Serveur natif
cargo build --locked --release -p flyff-server

# Client web : WASM, vérification TypeScript et bundle Vite
cargo xtask build-web

# Paquet Linux x86-64 avec client embarqué, depuis un environnement Linux adapté
cargo xtask dist --out dist/flyff
```

Le paquet contient l'exécutable, les scripts PNJ, un exemple de configuration et les notices de dépendances. Fournir le dossier `Resource/` séparément au serveur. Les sorties `target/`, `web/dist/` et `dist/` sont exclues de Git.

## Vérification

```bash
cargo fmt --all -- --check
cargo check --workspace --locked

# Construire le module WASM avant les vérifications web.
cargo xtask build-wasm --dev
pnpm --dir web typecheck
pnpm --dir web lint
```

Cette distribution contient uniquement les sources du jeu et l’outillage nécessaire à leur compilation et à la génération des données. Les suites de tests, fixtures, pages de démonstration, outils de suivi interne et configurations privées en sont exclus. Des commentaires du code renvoient encore à des documents de conception historiques non fournis.

## Organisation

| Dossier | Contenu |
| --- | --- |
| `crates/flyff-server` | Exécutable, HTTP/WebSocket, configuration, administration |
| `crates/flyff-game` | Monde, entités et systèmes de gameplay |
| `crates/flyff-sim`, `crates/flyff-rules` | Simulation et règles partagées |
| `crates/flyff-db` | SQLite, migrations et persistance |
| `crates/flyff-lua` | Exécution des scripts Lua |
| `crates/flyff-assets`, `crates/flyff-data`, `crates/flyff-formats`, `crates/flyff-vfs`, `crates/flyff-script` | Chargement et conversion des données du jeu |
| `crates/flyff-common`, `crates/flyff-proto`, `crates/flyff-wasm` | Types communs, protocole et interface WASM |
| `web/` | Client navigateur, interface et administration |
| `content/` | Scripts PNJ, compléments de quêtes et exemple de quiz |
| `proto/`, `schemas/` | Protocole réseau et schémas de données |
| `xtask/`, `tools/` | Compilation, génération et vérification |

Les fichiers générés du protocole, des constantes et des schémas sont conservés : ils sont nécessaires à la compilation. Les configurations génériques de compilation et le modèle de configuration du serveur sont inclus ; aucune configuration d’exploitation privée n’est fournie.

## Contributions

Ouvrez une issue ou une pull request sur [Laarm/FlyffRewriteRust](https://github.com/Laarm/FlyffRewriteRust). Décrivez le comportement concerné, les étapes de reproduction et les vérifications effectuées. Conservez les fichiers de verrouillage des dépendances.

Ne joignez pas de base de joueurs, journal contenant des données personnelles, secret d'exploitation ou archive des ressources originales.

## Licence

Le dépôt utilise la licence **Apache 2.0**, déjà définie sur le dépôt GitHub : voir [LICENSE](LICENSE). Les composants tiers conservent leurs propres licences, notamment [la police DejaVu](web/src/static/fonts/LICENSE-DejaVu.txt).

Flyff et ses ressources originales appartiennent à leurs ayants droit respectifs. Ce projet est indépendant ; la licence du dépôt n'accorde aucun droit sur les ressources originales fournies séparément.
