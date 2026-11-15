# passerelle — notes d'architecture (contrat v2)

**Fichiers** (765 lignes internes) : identite (types PasAppelant, PasEmpreinte) · routage (PasRoutes, PasService, PasEntree, PasRondes) · debit (PasDebit, PasBridage) · idempotence (PasRequetes) · messages (PasCanaux, PasMessages) · formats (PasFormats) · metier (PasMetier) · admin (PasAdmin, 19 routeurs).

## Choix de conception
- Temps : seul `PasEntree.traiter` lit l'heure (`now_ms`) ; les rondes passent par `ApiPasserelle.balayer` → `PasRondes.balayer` (par "systeme").
- Identité : jeton Bearer → `ApiAcces.authentifier` → personne ; org = `ApiAcces.employeur`, rôle = `ApiAcces.role_de`. `par` = cette personne, `valideur` = personne d'un SECOND jeton (`X-Jeton-Valideur`), jamais le corps. "systeme" refusé à l'entrée (401).
- Propriété 5 : classe de débit "urgence" (réponse de coordination, accusé d'escalade, accusé de message) jamais comptée, bridée ni coupée par le mode ; `PasRoutes.declarer` refuse de la classer ailleurs. Message non livré / non acquitté à l'échéance (niv. 3 : 5 min + 15 min) ⇒ `en_defaut` ⇒ `ApiEscalade.escalader` (r ≠ "", opérateur) ou `ApiConformite.constater` / `constater_etat` ; sans canal actif ⇒ défaut immédiat.
- Propriété 6 : toute donnée d'objet sortante (CDM, alerte, lectures API) passe par `ApiDonnees.peut_recevoir` objet par objet (masqué sinon) et `ApiDonnees.tracer` ; `PasMessages.du_rapprochement` dit qui a été averti de quoi et quand.
- Idempotence obligatoire sur POST/DELETE, par organisation : un réessai de réponse à une alerte ne crée pas une seconde décision.
- Routes métier : table fermée (`PasCible`) → 5 aiguillages ; habilitation fine jugée par la façade appelée. Gestes de la passerelle câblés hors table.

## Règle interne (dans les cadres)
`PasMessages en_file->en_envoi when PasCanaux.actif via canal_de`. Les règles sur PasSessions/PasJetons/PasRequetes ont disparu avec les sessions (acces).

## Règles entre sous-systèmes proposées
- Cible = PasCanaux.actif ; exige = ref:organisation existante non radiée ; lien = PasCanaux.org_de
- Cible = PasMessages.escalade ; exige = escalade:escalade ouverte (échelon ≥ 1) ; lien = PasMessages.rapprochement_de
- Cible = operateurs:actif (admission) ; exige = PasCanaux.actif (un canal vérifié) ; lien = à écrire chez operateurs (op → canal principal)
- Cible = coordination:arbitrage pour silence ; exige = PasMessages.en_defaut|livre (l'alerte est partie) ; lien = coordination.alerte_de (r → message)

## Phase 1b (contrat v2)
- **Façade : 5/5 signaux branchés** (facade.txt) : request, notifier, acquitte, echoue (PasMessages.echoue), balayer (PasRondes.balayer).
- **Horloge retirée** : PasHorloge, `every` et `now()` supprimés ; `now_ms` reste seulement dans PasEntree.traiter (autorisé).
- **Sessions humaines → acces** : PasCles, PasSessions et PasJetons supprimés (−266 lignes, d'où un total sous 900). Routes `session.ouvrir|clore` → `ApiAcces.ouvrir_session|fermer_session`. Pas de clés machine à machine : un automate = un compte de personne chez acces.
- **notifier court** : ApiPasserelle.notifier → PasFormats.notifier → PasMessages.mettre_en_file → {PasCanaux.principal, PasCanaux.adresse, http_post, Journal.noter} : 4 niveaux, aucune autre façade. La composition CDM/alerte (diffusion, trace) se fait à l'envoi, dans le balayage. `r` est un paramètre ; la convention « FORMAT:CJ-n » est abandonnée. Un destinataire inconnu n'est plus vérifié chez ref (Inconnu = forme ≠ "ORG-…") : un opérateur sans canal produit un défaut, donc une escalade, jamais un refus silencieux.
- **Trous fermés** : CDM avec `ApiConjonctions.distance` et `mis_a_jour` ; lecture de plan contrôlée sur `ApiManoeuvres.objet_de` ; `ApiCoordination.designe(r, par)` (diffusion jugée par coordination) et `en_attente_de` (route `coordination.en_attente`) ; `ApiRef.genre` (rôles admissibles d'un canal ; défaut d'un État → `constater_etat`) ; route capteurs `mesure.deposer` → `ApiObservation.deposer_mesure` ; `registre.immatriculer` (autorité d'État) ; `deposer` d'éphéméride avec `position` (premier point de l'OEM) ; `anomalie` avec `genre`.
- **Routeurs d'administration** : ajout de fermer_session, constater_etat, constater_collision, exempter, renforcer, retirer, constater_non_annoncee, deposer_mesure, attribuer, immatriculer, changer_regime, changer_tutelle.

## Phase 1b — demandes
- Rien de bloquant. Souhait : `ApiPasserelle.notifier` gagnerait un `format: String` (le CDM se reconnaît aujourd'hui à `sujet == "CDM"`).

## ⚠ attendus (hors « hole »)
- `ensures` sur des types String nominaux : vérifiés à l'exécution seulement (limite de Soma) ; bornes de taille des listes : `require len(…) < N` dans les corps.
- Les slots `via` / écrits une fois (messages) croissent sans borne : archivage à concevoir.

## Phase 2 — règles entre sous-systèmes (operateurs + passerelle)
- Écrites (4) :
  1. OpChangements.approuve ⇐ RefEtat.participant via etat_accueil — arête (approuver ; + ApiRef.etat_participant).
  2. OpExercices.reussi ⇐ PasMessages.acquitte via message_de — arête (accuser ; + ApiPasserelle.acquitte, reads message_de).
  3. PasCanaux.actif ⇐ RefOrg.inscrite || RefOrg.active via org_de — arête (verifier ; + ApiRef.organisation_existe).
  4. OpAstreinte.en_service ⇐ AccesPersonnes.active via OpAstreinte.personne — toml (sys/operateurs/regles.toml : arête déjà `when OpEngagements.en_vigueur`).
- Verdicts (verify ×2 sur une copie avec regles.toml fusionné) : les 4 en « open obligation … written by the hole » (lien écrit par
  declarer/lancer/planifier, jugé au corps) ; aucun ✗ ni « not explored » sur operateurs/passerelle. Seul ✗ du run : EscVeille.couverte (escalade, hors périmètre).
  check 0 erreur ; cycles.py : aucune composante, profondeur max 23.
- Écartées :
  - OpMembres.actif ⇐ RefOrg.active : toml essayé → ⚠ « a run enters actif without it » (activer depuis RefOrg=inscrite : cadre de trou) ;
    admis->actif porte déjà un when, suspendu->actif est un retour arrière. Contrôle au corps : ApiRef.organisation_existe ajouté à activer/retablir.
  - OpLicences.en_vigueur ⇐ RefEtat.participant via etat_licence : lien réécrit par declarer et transferer (pas écrit une fois).
  - OpDossiers.accepte ⇐ donnees:adhérent : arête déjà prise ; adhésion "AD-n" inconnue au dépôt et renouvelée sous un nouvel id ; corps : ApiDonnees.adherent.
  - OpMembres.actif ⇐ PasCanaux.actif : canal principal variable, choisi par l'org, aucun lien écrit une fois.
  - Suspension "conformite" ⇐ ConfDemande.acceptee, retablir ⇐ ConfMesure.levee||echue : suspensions répétées, aucun lien op → demande/mesure écrit une fois.
  - PasMessages.escalade ⇐ escalade ouverte : dossier "ESC-n" inconnu de passerelle (escalader rend l'échelon), et e1|e2|e3|e4 = 4 alternatives.
- Structure : aucun état ni slot ajouté ; calls et commentaires (Refus EtatInvalide) mis à jour sur approuver, accuser, verifier, activer, retablir, prendre_service.

## Phase 3 — corps
- **Signaux remplis : 73/73** (12 cells internes + ApiPasserelle 5/5), aucun trou ; 2 001 lignes de corps (chair/*.cell, dont 346 générées pour les 19 routeurs d'admin) ; doublures 271 l. ; `$S test test_passerelle.cell` : **25 tests, 370 assertions, toutes vertes**, sans appel réseau réel (http_post et random_token mockés). check 0 erreur ; cycles.py : aucune composante.
- **Cadres corrigés (amendés)** : `PasCanaux.est_actif(canal) -> Bool reads vie` ajouté (getter de la règle `en_file -> en_envoi when PasCanaux.actif via canal_de`, que traiter_file ne pouvait pas juger) ; `PasMessages.traiter_file` : + `calls PasCanaux.est_actif`, + `reads sujet_de` (le sujet manquait aux réessais).
- **Demandes à l'intégrateur** : aucune bloquante. Souhaits : (1) `PasCanaux.suspendre` + `calls ApiRef.genre` (il saisit aujourd'hui `ApiConformite.constater` même pour un État, sous `try`) ; (2) souhait de 1b maintenu (`notifier` avec `format`).
- **Défauts de conception relevés** : PasReservation n'a pas de champ pour la clé effective après abandon (« #n ») → portée par `corps` pour l'issue "nouvelle" ; « débit divisé par 4 » au cran bride : `consommer` n'a pas l'état de bridage → PasEntree fait coûter 4 jetons ; « http_post signé » sans secret : le corps porte l'empreinte SHA-256 ; messagerie sécurisée servie par un relais https codé en dur ; traiter_file balaie tous les messages (livrer_avant) pour trouver livrés échus / défauts : O(n) par battement (archivage, déjà noté) ; ordre imposé par la fiche : un POST vers un sous-système dégradé rend 422 Suspendu avant le contrôle d'Idempotency-Key.
- **Traductions HTTP retenues** : HorsBornes « champ … », OutOfRange (type nominal) et `requires` d'une cible → 400 ; Habilitation/QuatreYeux/Confidentiel → 403 ; « geste … » Inconnu → 404 ; autre tag → 422 ; imprévu → abandon + 422 EtatInvalide (kind seul dans le message).
- **Limites de Soma rencontrées** : types nominaux stricts (Org/Horodatage/PasEmpreinte à convertir partout, helpers typés nominalement) ; accolades littérales = `{{ }}` ; un refus annule aussi le Journal.noter qui le précède (routeurs, acquitter Echu) → la trace du refus est la ligne finale de PasEntree ; `get_status` d'une instance jamais vue rend l'état initial, pas `()` ; une valeur hors type passée à une cible lève `requires` (pas OutOfRange) ; les doublures ne peuvent appeler aucune autre façade (cadre) → conventions par chaînes.
