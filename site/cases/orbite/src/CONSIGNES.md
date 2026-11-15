# ORBITE COMMUNE — consignes aux architectes de sous-système

ORBITE COMMUNE est un système mondial de coordination du trafic spatial, écrit en Soma,
**squelette d'abord**. Aujourd'hui, quand deux satellites risquent de se percuter, les opérateurs
s'appellent et décident au cas par cas qui manœuvre ; beaucoup sont injoignables ; il n'existe
aucun protocole commun. ORBITE est ce protocole, neutre, ouvert et prouvé : catalogue, éphémérides,
criblage, coordination (une seule décision par rapprochement), manœuvres criblées, escalade jusqu'aux
États, rentrées, fin de vie, responsabilité. Tout est simulé : États, opérateurs et objets fictifs.

Cette phase ne produit QUE des squelettes : mémoire, invariants, machines à états, signaux avec leur
cadre et leur commentaire. Aucun corps (`flesh`).

Binaire : `S=~/fun/soma-banc/binaires/soma-v3.2.4-cone` (n'utilise PAS le `soma` du PATH : seul
celui-ci prouve vite les règles entre machines). Lis `$S docs skeleton` puis `$S docs agent`
(sections utiles) avant d'écrire. Travaille dans `~/fun/orbite`.

## Ce qui doit être VRAI à la fin (les propriétés qui justifient le projet)

Chaque architecte se demande lesquelles le concernent et rend sa structure capable de les porter
(états explicites, liens écrits une fois, getters de façade) :

1. Un rapprochement a au plus UNE désignation arrêtée à la fois ; jamais deux manœuvres
   d'évitement approuvées pour le même rapprochement.
2. Un objet sans propulsion, hors service ou en anomalie n'est jamais désigné pour manœuvrer.
3. Un objet habité ne manœuvre que s'il est seul capable de le faire.
4. Aucune manœuvre n'est approuvée sans criblage de la trajectoire après manœuvre, ni sur une
   éphéméride périmée, ni par un opérateur suspendu.
5. Toute alerte de niveau 3 reçoit une réponse avant son échéance, ou elle est escaladée ; un
   opérateur silencieux ne bloque jamais le système (l'arbitrage d'ORBITE reprend la main).
6. Une décision prise est traçable jusqu'aux données qui la fondaient (preuve en cas de dommage),
   et ces données ne sont communiquées qu'aux organisations autorisées.

## Ce que tu possèdes

- Le dossier `sys/<toi>/`, sauf `api.cell` (la façade, figée : tu ne la modifies pas).
- Tu y écris autant de fichiers `.cell` que tu veux (un par grand thème : 3 à 8 fichiers).
- Tu ajoutes un `use sys::<toi>::<fichier>` par fichier à la fin de `sys/<toi>/entree.cell`.
- Tu écris `sys/<toi>/facade.txt` : pour chaque signal de ta façade, une ligne
  `ApiX.signal: calls Cell.sig, Cell.sig2[, moves …]` — les signaux internes auxquels il délègue.
- Tu écris `sys/<toi>/NOTES.md` (≤ 40 lignes) : choix de conception, règles entre
  sous-systèmes que tu proposes (voir plus bas), getters de façade qui te manquent, points ouverts.

Tu n'ouvres NI ne modifies rien d'autre : ni `app.cell`, ni `commun/`, ni les dossiers des
autres sous-systèmes (sauf leur `api.cell`, en lecture).

## Ce que tu peux appeler

- tes propres cells ;
- les signaux des 20 façades `Api*` (le contrat entre sous-systèmes) ;
- `Journal.noter` et les types de `commun/types.cell`.
Jamais une cell interne d'un autre sous-système.

## Exigences de qualité

1. **Types partout** : `Horodatage`, `Duree`, `Jour`, `Distance`, `DeltaV`, `Masse`, `Probabilite`,
   `Pourcent`, `Org`, `Etat`, `Objet`, `Rapprochement`, `NatureObjet`, `Propulsion`, `Regime`,
   `Niveau`, `Gravite`, `Qualite`, `Diffusion`, `Role`, `SousSysteme`. Une valeur à liste fermée
   devient un type `cell type X = String where …` déclaré dans TON fichier, préfixé par ton
   sous-système (`ConjEtatCriblage`, `ManMotif`…). Préfixe aussi tes cells (`Conj…`, `Man…`).
2. **Chaque signal a un commentaire** qui dit ce qu'il fait, ce que veut dire sa réponse, les
   unités, et `Refus : …` avec les tags du type `Refus` de `commun/types.cell` (et seulement eux).
   Une fiche doit suffire à écrire le corps sans rien demander.
3. **Habilitations** : toute action humaine prend `par: String` (et `valideur: String` si quatre
   yeux) ; le commentaire dit quel `Role` est exigé ET sa portée (rôle d'ORBITE, ou rôle de
   l'opérateur propriétaire via `ApiAcces.agit_pour`). Contrôle via `ApiAcces.a_le_role` /
   `ApiAcces.agit_pour` / `ApiAcces.quatre_yeux` (mets-les dans `calls`). L'identité interne
   des traitements automatiques est `par = "systeme"`.
4. **Machines** : chaque cycle de vie important est une `state` ; chaque machine atteint un état
   terminal depuis tout état, ou est réactive (revient à son initial). Nomme les arêtes dans les cadres.
5. **Règles entre machines** de ton sous-système : sur l'arête, dans le cadre
   (`moves a -> b when AutreCell.etat [via slot]`). Un slot `via` est écrit UNE fois, par un seul
   signal, à la création, jamais supprimé. Une arête ne porte qu'UN `when`. Évite les conditions
   sur une arête qui revient en arrière et les alternatives à 3 états ou plus (explosion de verify).
6. **Invariants** sur la mémoire (bornes, positivité, tailles avec `const MAX_…`), y compris entre
   slots quand c'est le sens métier.
7. **Temps** : toujours un `h: Horodatage` fourni par l'appelant (jamais `now()`), échéances en
   millisecondes ; un délai dépassé est un état ou un refus `Delai`, pas un oubli.
8. **Pas de remplissage** : pas de commentaires creux, pas de signaux sans usage, pas de
   duplication. Les lignes comptent parce qu'elles portent de la structure.
9. **Les trous de contrôle sont des bugs** : pour chaque contrôle, demande-toi ce qui y échappe
   (un objet jamais catalogué, un opérateur silencieux, une donnée périmée, un état oublié).
10. **Gestes d'administration** : un signal interne qui prend `par` et que ta façade n'appelle pas
    sera routé par la passerelle (`/admin/<sous-système>/<Cell.signal>`) ; son commentaire doit
    donc suffire à un client externe.

## Règles entre sous-systèmes

Tu ne peux pas les écrire toi-même (elles nomment des machines d'autres sous-systèmes). Propose-les
dans `NOTES.md`, forme : `Cible = MaCell.etat ; exige = <sous-système>:<fait> ; lien = slot`.
Elles seront écrites en phase 2.

## Contrôle (obligatoire avant de rendre)

    $S check sys/<toi>/entree.cell
    perl -e 'alarm 300; exec @ARGV' $S verify sys/<toi>/entree.cell --summary
    wc -l sys/<toi>/*.cell

`check` sans erreur ; `verify` sans ✗ (les ⚠ « hole » sont normaux pour un squelette ; tout autre ⚠
sur tes cells est à expliquer dans NOTES.md). Lance `verify` au plus TROIS fois (20 agents
partagent la machine). Cible : **900 à 1 100 lignes** de squelette interne (hors `api.cell` et
`entree.cell`). Pas de git.

## Périmètres

| Sous-système | Doit couvrir |
|---|---|
| ref | États participants (adhésion, suspension), organisations (État, opérateur, fournisseur de capteurs) et leur État de tutelle, catalogue des objets (numérotation, nature, propriétaire, État d'immatriculation, régime, propulsion, masse), cycle de vie de l'objet, changements de propriétaire (cession d'un satellite en orbite, accord des deux États), registre au sens de la Convention de 1975 (délais d'immatriculation), historique des modifications avec quatre yeux |
| acces | personnes, rôles et leur portée (ORBITE / opérateur / État), rattachement, sessions et jetons, quatre yeux, accès d'urgence avec expiration et revue a posteriori, recertification périodique, départs |
| operateurs | dossiers d'adhésion, licence nationale (vérification auprès de l'État, expiration, retrait), engagements (délais de réponse par niveau), astreinte 24/7 (planning, relèves, trous de planning), exercices de joignabilité, statut (suspension, exclusion, retrait), changement de contrôle |
| ephemerides | dépôts (validation de format, époque, horizon, cohérence avec l'orbite observée), fraîcheur par régime et en tempête, qualité de covariance, manœuvres incluses, invalidation, historique, quotas de dépôt |
| observation | réseau de capteurs (radar, optique, laser ; fournisseurs ; étalonnage ; pannes), tâches de suivi et leur priorité, mesures et leur corrélation, détermination d'orbite, pistes non corrélées (nouveaux objets), détection de manœuvres non annoncées, capacité |
| conjonctions | campagnes de criblage (tout-contre-tout, par régime, cadence), rapprochements (création, mises à jour, niveau, seuils), messages de conjonction, criblage des plans de manœuvre, criblage renforcé (nuage, tempête), clôture (résolu, passé, collision) |
| coordination | coordination par rapprochement (ouverture, proposition par défaut, réponses, accord, désaccord, échéances, arbitrage avec quatre yeux, rouverture), verrou de désignation unique, journal des échanges, coordinations multi-objets (un objet dans plusieurs rapprochements simultanés) |
| priorites | versions des règles de priorité (rédaction, consultation des États, vote, adoption, entrée en vigueur, abrogation), table de décision, préavis par propulsion, dérogations (demande, instruction, octroi, expiration), cas particuliers (habité, essaim, propulsion faible) |
| manoeuvres | plans (évitement, maintien à poste, transfert, désorbitation), criblage obligatoire, approbation, fenêtres d'exécution, exécution confirmée, vérification après manœuvre par observation, anomalies, annulations, manœuvres non annoncées constatées |
| escalade | échelons et délais (fonction du temps restant au TCA), relances, contacts successifs (astreinte, chef de mission, point focal, direction), accusés, clôture, escalades diplomatiques, statistiques par opérateur |
| reentrees | campagnes de rentrée (fenêtres successives, rétrécissement), risque au sol, États survolés et leur notification, rentrées contrôlées (zone d'impact océanique, approbation), constat, débris retrouvés et restitution à l'État de lancement (Accord de 1968) |
| lancements | préavis, fenêtre et criblage COLA, avis d'ORBITE, report, lancement, injection, déploiement des charges utiles, identification et catalogage des objets (dont les étages), délai d'immatriculation, échecs |
| fin_de_vie | plans de fin de vie (dépôt, revue, approbation), fin de mission, passivation, désorbitation ou orbite cimetière, échéance et retard, missions de retrait actif (consentement du propriétaire ET de l'État, autorisation, capture, constat) |
| evenements | fragmentations, collisions confirmées, anomalies d'objets (déclaration, suivi, levée), nuages de débris (estimation, catalogage progressif des fragments, régime touché), notifications aux États et aux opérateurs |
| responsabilite | dossiers de preuve (gel des données et décisions), réclamations entre États (dépôt, voie diplomatique, commission de règlement, sentence), responsabilité pour faute (décisions de coordination ignorées), assurance, conservation et purge des données |
| donnees | accord de partage (versions, adhésion, retrait), niveaux de diffusion par objet, demandes d'accès, consentements, traces de communication, publication publique (catalogue ouvert, rapprochements anonymisés), embargo pour objets sensibles |
| meteo | sources (indices Kp, F10.7), mesures et prévisions, alertes de tempête (émission, mise à jour, fin), facteur d'incertitude par régime, déclenchement du criblage renforcé, historique |
| conformite | manquements (constat, contradictoire de l'opérateur, qualification, clôture), notation glissante, mesures (avertissement, surveillance renforcée, demande de suspension à l'État), rapports annuels aux États, indicateurs publics |
| passerelle | sessions API, authentification par jeton, limites de débit, routage vers les façades et routes d'administration, idempotence, messages sortants (file, priorité par niveau, réessais, accusés, échecs ⇒ escalade), formats (CDM, OEM, alerte) |
| operations | incidents du système (cycle de vie, gravité, post-mortem), cellule de crise, mode dégradé, bascule de secours, changements (demande, approbation, gel pendant les urgences, déploiement, retour arrière), tests de continuité |
