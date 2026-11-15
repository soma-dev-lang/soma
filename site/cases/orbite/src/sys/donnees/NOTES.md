# donnees — notes d'architecture

## Choix de conception
- 8 fichiers, 12 cells, 8 machines : DonAccords (version), DonAdhesions (adhesion), DonEmbargos (embargo),
  DonDemandes (demande), DonConsentements (consentement, id = demande), DonPartages (partage bilatéral),
  DonPublications (publication), DonViolations (violation). Sans machine : DonDiffusion, DonControle, DonTraces, DonBalayage.
- Point de décision unique : DonControle.fondement(org, objet, r, h) renvoie le FONDEMENT ("proprietaire",
  "etat_immatriculation", "orbite", "partie:CJ", "urgence:CJ", "public", "adhesion:AD", "bilateral:PB", "demande:DA")
  ou "". peut_recevoir = fondement ≠ "". DonTraces.tracer enregistre le fondement avec chaque envoi (propriété 6).
- Sécurité d'abord : un rapprochement non clos rend l'objet au moins "parties" ; embargo et suspension d'adhésion
  ne coupent jamais le fondement "partie" ; une partie non adhérente reçoit en niveau 3 ("urgence").
- "restreint" n'est atteint que par embargo (accordé à quatre yeux, ≤ 365 j) ; le plafond par nature est fixé par la
  politique (débris publics par défaut). Embargo ⇒ demandes accordées révoquées, objets exclus des publications.
- Les contrôles horaires (demande, partage) lisent `fin` contre `h` : justes avant même le balayage.
- Accord : une version en vigueur à la fois ; adoption = 2/3 des avis exprimés des États, ≥ 30 j de consultation ;
  adhésion à une version remplacée valable 90 j. Suspension sur violation de gravité 3 ; opérateur suspendu/exclu
  chez operateurs : perd "adhesion", garde "partie".
- Règles internes (arêtes) : adhesion signee→active ⇐ DonAccords.en_vigueur via version_de ; demande deposee→instruite
  ⇐ DonAdhesions.active via adhesion_de ; demande instruite→accordee ⇐ DonConsentements.donne (même id) ;
  partage propose→actif ⇐ DonAdhesions.active via adhesion_de ; publication relue→publiee ⇐ DonAccords.en_vigueur
  via version_de. Chaque slot via a un seul écrivain (signer, deposer, proposer, preparer).

## Règles entre sous-systèmes proposées
- Cible = DonEmbargos.actif ; exige = ref:objet catalogué (etat_objet ∉ inconnu, rentre, retire) ; lien = DonEmbargos.objet_de
- Cible = DonAdhesions.active ; exige = ref:organisation inscrite non radiée ; lien = DonAdhesions.org_de
- Cible = DonDemandes.accordee ; exige = ref:objet catalogué ; lien = DonDemandes.objet_de
- Cible = DonViolations.constatee ; exige = conformite:manquement constaté ; lien = DonViolations.manquement (écrit par qualifier)

## Avertissements
- ⚠ invariants runtime-checked (tailles, bornes de trous, `?? 0` entre slots : écrire le slot de droite d'abord) :
  normaux pour un squelette. ⚠ ensures sur retours String typés (Diffusion) : runtime-checked par conception.
- ⚠ 4 open obligations « slot via écrit par un trou » (version_de ×2, adhesion_de ×2) : jugées aux corps.

## Phase 1b — contrat v2
- Façade : 5/5 signaux branchés (facade.txt) ; balayer → DonBalayage.balayer (entrée en vigueur, adhésions, embargos,
  consentements, demandes, partages). Aucun every / now : le temps n'entre que par balayer(h).
- Trous fermés : publication lit ApiRef.catalogue (paginé) et ApiConjonctions.clos_entre (plus de liste manuelle) ;
  purge des traces seulement si ApiResponsabilite.purge_autorisee(r, OBJ, J:<jour>) ; signer contrôle le rôle selon
  ApiRef.genre (ORBITE ne signe pas) ; objet non attribué : propriétaire ORBITE, État "" sauté dans fondement.
- Demandes : aucune.

## Phase 2 — règles entre sous-systèmes (donnees + operations)
- Écrites : 4 (3 arête, 1 toml).
  (1) DonEmbargos.actif ⇐ RefObjet.catalogue || RefObjet.en_service via objet_de — arête demande -> actif (accorder ; + calls ApiRef.etat_objet ; refus EtatInvalide).
  (2) DonAdhesions.active ⇐ RefOrg.inscrite || RefOrg.active via org_de — toml (sys/donnees/regles.toml ; arête signee -> active déjà prise par DonAccords.en_vigueur, et suspendue -> active est une arête retour) ; contresigner et retablir + calls ApiRef.organisation_existe, refus EtatInvalide.
  (3) OpsGelUrgence.arme ⇐ ConjRapprochement.alerte || en_coordination, même id — arête veille -> arme (armer ; + calls ApiConjonctions.etat).
  (4) OpsChangement.en_deploiement ⇐ AccesPersonnes.active via demandeur — sur les deux arêtes approuve / approuve_urgence -> en_deploiement (deployer ; + calls ApiAcces.role_de ; refus EtatInvalide).
- Écartées : DonDemandes.accordee ⇐ objet catalogué (arête prise ; accès aux données d'un objet rentré/fragmenté légitime pour preuve et réclamation ; existence contrôlée par deposer) ;
  DonViolations.constatee ⇐ manquement constaté (le manquement est créé par qualifier lui-même : tautologique ; lien écrit à la qualification, pas à la création ; aucun getter d'état de manquement dans ApiConformite) ;
  OpsGelUrgence.leve ⇐ rapprochement resolu|passe|collision (3 alternatives, et contredit la sortie voulue « TCA + 1 h » : conjonctions en panne ne doit pas geler les changements indéfiniment) ;
  OpsIncident.clos ⇐ dossier gelé (conditionnelle à effet "faussees", arête prise, lien écrit par geler_preuves après création : reste au corps) ;
  OpsCrise.active ⇐ pilote actif (pilote = par d'activer, inconnu à la création ; ApiAcces.a_le_role exige déjà une personne active) ;
  purge des traces ⇐ RespPurges.autorisee (DonTraces sans machine ; corps via ApiResponsabilite.purge_autorisee).
- verify (1 passage) : VERIFY OK, aucun ✗ ni « not explored » ; (3) ✓ ; (1) et (4) « open obligation … written by the hole » (objet_de par demander, demandeur par soumettre) ; (2) toml non vérifiable avant fusion dans soma.toml.
- Structure : aucun état ni slot ajouté ; commentaires de slot (demandeur écrit une fois par soumettre) et fiches mises à jour ; retablir lit org_de. cycles.py : aucune composante, profondeur max 23.

## Phase 3 — corps
- Signaux remplis : 88/88 (12 cells internes + ApiDonnees 5/5), 1 helper (`DonControle._niveau`) ; ~1 330 lignes dans `chair/` (9 fichiers).
- Tests : `test_donnees.cell`, 21 tests / 308 assertions, tous verts (cycles de vie des 8 machines, refus Habilitation/QuatreYeux/portée/EtatInvalide, scénarios demande d'accès → consentement → trace → retrait, et violation → suspension). Doublures sans mémoire : `doublures/donnees.cell` (ApiAcces, ApiRef, ApiConjonctions, ApiOperateurs, ApiPasserelle, ApiOperations, ApiConformite, ApiResponsabilite).
- Cadres corrigés (amendés, `check app.cell` OK) : DonAccords.abandonner + reads empreinte ; DonAdhesions.signer + reads adhesion ; DonDemandes.deposer + reads demande ; DonDemandes.instruire et DonPartages.accepter + calls DonAdhesions.etat (règle d'arête `when DonAdhesions.active via adhesion_de` sans getter) ; DonConsentements.solliciter + calls ApiRef.orbite ; DonPublications.annuler + reads genre_de ; DonViolations.clore + reads en_cause ; DonBalayage.balayer + calls DonAccords.en_vigueur (compter la mise en vigueur).
- Défaut de conception corrigé : 3 invariants `(a ?? 0) <= (b ?? 0)` interdisaient la création (b absent à la création) → `(b ?? 9999999999999)` sur DonAdhesions (signee_le/caduque_le), DonDemandes (deposee_le/fin), DonPublications (prepare_le/publiee_le).
- Choix de corps : DonDemandes.conclure refuse la demande (état "refusee") quand l'objet est passé sous embargo au lieu de lever Confidentiel (lever annulerait le consentement de repondre) ; ouvrir_resignature/echoir couvrent aussi les adhésions suspendues (arête suspendue -> caduque) ; qualifier d'une fuite d'ORBITE : incident seul (pas de constater ni de suspension) ; suspension g3 sous `try` si l'organisation n'a plus d'adhésion active ; propriétaire "inconnu"/"" traité comme ORBITE.
- Demandes à l'intégrateur : aucune modification de façade. À noter : ApiRef.proprietaire dit « "inconnu" pour un objet non attribué » mais cataloguer dit ORBITE — les corps acceptent les deux.
- Trous laissés : aucun. Pas de noyau natif (le criblage concerne conjonctions).
- Limites de Soma : typage strict des types raffinés (Org/Objet/Etat/Role) sur chaque appel et écriture de slot (≈ 60 conversions) ; un `transition(id, src, cible_calculée)` est refusé (écrire les cibles en clair) ; avertissement `?? 999…` persistant sur les invariants corrigés.
