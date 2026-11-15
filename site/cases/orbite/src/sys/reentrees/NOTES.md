# reentrees — notes d'architecture

## Choix de conception
- Une campagne par rentrée ("RE-<n>") ; son id est aussi celui de ReeRisque et de ReeConstat : règles `when` sans lien
  (ReeRisque.fige ⇐ ReeCampagne.rentree|annulee ; ReeCampagne.close ⇐ ReeConstat.sans_suite|transmis).
- ReeNotification ("NT-<n>", une par campagne × État) et ReeControlee ("RC-<n>") suivent la campagne par `campagne_de`,
  écrit une fois à la création (3 règles `via` : envoi seulement si campagne ouverte|imminente ; approbation de la
  rentrée contrôlée idem ; « reussie » seulement si campagne rentrée).
- Délais = états : campagne « echue » (fin + 6 h sans constat), notification « relancee » puis « sans_accuse » (un État
  silencieux ne bloque rien, ORBITE est avertie), débris « non_identifie » / « conserve », constat « sans_suite » après
  le délai de réclamation d'un an. `ReeCampagne.balayer` (par "systeme") porte échéances, cadence de révision
  (24 h, 3 h en phase finale) et relances.
- Risque cliquet : « a_risque » ne redescend que par `declasser` à quatre yeux ; à risque + objet manœuvrant ⇒ demande
  de rentrée contrôlée au propriétaire.
- Diffusion (propriété 6) : avis complet si ApiDonnees.peut_recevoir (puis tracer), sinon avis réduit (fenêtre, zone,
  risque) — un État survolé est toujours averti. Fondement de chaque révision (dépôt d'éphéméride, Kp, suivi) gardé,
  traces de survol historisées, fondement de l'approbation d'une rentrée contrôlée gardé.

## Règles entre sous-systèmes proposées
- Cible = ReeControlee.approuve ; exige = manoeuvres:plan "crible" ; lien = ReeControlee.manoeuvre_de
- Cible = ReeControlee.executee ; exige = manoeuvres:plan "execute" ou "verifie" ; lien = ReeControlee.manoeuvre_de
- Cible = ReeConstat.transmis ; exige = responsabilite:dossier gelé pour l'évènement (id de campagne) ; lien = id de campagne
- Cible = ReeDebris.identifie ; exige = ref:objet "rentre"|"fragmente" ; lien = ReeDebris.objet_attribue (à écrire une fois)
- Inverses (cible chez eux) : ref:objet "rentre" exige ReeCampagne.rentree (lien objet → ReeCampagne.courante) ;
  conjonctions:rapprochement « resolu » d'un objet rentré ; fin_de_vie:désorbitation constatée exige ReeCampagne.rentree.

## Phase 1b — contrat v2
- Façade : 9/9 signaux branchés (facade.txt). Horloge : aucun every/now ; ApiReentrees.balayer → ReeCampagne.balayer,
  qui enchaîne échéances et cadence des campagnes, relances (ReeNotification.relancer), ReeConstat.balayer,
  clôture rentree → close, ReeDebris.balayer. Les gestes manuels doublons (clore, clore_sans_suite,
  abandonner_identification, conserver) sont supprimés.
- Trous fermés : F10.7 prévu dans le fondement (ApiMeteo.f107_prevu) ; débris restitués aux
  ApiLancements.etats_de_lancement (demandeur gardé) ; rentrée à risque majeur ⇒ tous les ApiRef.etats_participants
  (motif "majeur", ApiRef.masse) ; plan lié vérifié par ApiManoeuvres.objet_de/motif_de ("desorbitation") ;
  État sans accusé ⇒ ApiConformite.constater_etat ; anomalie couverte par ApiRef.peut_manoeuvrer ; notifier avec r = "".
- Ordre avec manoeuvres : criblage du plan → ReeControlee.approuve (exige "crible") → manoeuvres approuve sur
  controlee_approuvee (plus d'exigence "approuve" ici, qui aurait fait un cercle).

## Phase 1b — demandes
- Aucune bloquante. Suggestion : observation appelle ApiReentrees.prevoir quand le périgée observé descend.

## Phase 2 — règles entre sous-systèmes (reentrees + lancements)
Écrites : 7 (verify, toml fusionné dans une copie : aucun ✗ ni « not explored » sur ces cells ; les 7 sortent
« open obligation … written by the hole » = lien écrit par un signal sans corps, jugé au corps).
- ReeControlee.approuve ⇐ ManPlan.crible || ManPlan.approuve via manoeuvre_de — toml (sys/reentrees/regles.toml ; arête déjà prise par ReeCampagne)
- ReeControlee.executee ⇐ ManPlan.execute || ManPlan.verifie via manoeuvre_de — arête (confirmer)
- ReeConstat.transmis ⇐ RespDossiers.gele via dossier — arête (transmettre, seul écrivain de `dossier` ; + calls ApiResponsabilite.sous_gel)
- ReeDebris.identifie ⇐ RefObjet.rentre || RefObjet.fragmente via objet_attribue — arête (identifier, seul écrivain)
- LancLancement.autorise ⇐ OpMembres.actif via op_de — toml (sys/lancements/regles.toml ; arête prise par LancAvis.favorable ; + calls ApiOperateurs.est_actif, refus Suspendu)
- LancPiece.cataloguee ⇐ RefObjet.catalogue via objet_de — arête (nouveau LancPiece.lier)
- LancPiece.immatriculee ⇐ RefRegistre.immatricule via objet_de — arêtes (immatriculer et relever_retards, 4 arêtes)
Structure : LancLancement.creer (nouveau, seul écrivain de op_de, appelé par notifier et enregistrer_non_notifie) ;
LancPiece.lier (nouveau, seul écrivain de objet_de/origine_de, appelé par cataloguer et rattacher : rattacher exige
désormais l'objet « catalogue » chez ref). Aucun état ajouté. cycles.py : aucune composante, profondeur max 23.
Écartées : LancEnquete.close ⇐ dossier gelé si dommages (conditionnelle ; ApiResponsabilite n'expose pas le dossier
d'un évènement et geler refuse Existe ⇒ lien inconnu) ; LancPiece.identifiee (étage) ⇐ plan de fin de vie conforme
(conditionnelle au genre, voulue non bloquante : manquement) ; LancLancement.identifie ⇐ RefRegistre (proposition ref :
pas d'objet unique par lancement ; transposée à la pièce il faudrait ≥ 3 alternatives a_immatriculer|en_retard|
immatricule|dispense). Inverses (cibles chez ref, conjonctions, fin_de_vie, manoeuvres) : hors périmètre.
Verify : 1 seul ✗ global, EscVeille.couverte (escalade, pas à moi).

## Phase 3 — corps
- Signaux remplis : 59/59 (7 cells internes + ApiReentrees 10/10) ; ~1 140 lignes de corps (sys/reentrees/chair/*.cell).
- Tests : test_reentrees.cell, 13 tests / 263 assertions, tous verts (`$S test test_reentrees.cell`) ; doublures/reentrees.cell
  (13 façades, pures). Couverts : cycle campagne ouverte→imminente→rentree→close, échue→annulée→rouverte, risque cliquet +
  déclassement 4 yeux + majeur, notifications envoi/accusé/relance/sans_accusé/levée, survol, rentrée contrôlée réussie,
  manquée après report, refus (habilitation, portée, 4 yeux, état, délai, préavis), constat→dommages→transmis, débris.
- Cadres amendés (une commande --amend, `check app.cell` propre) : ReeControlee.approuver + `calls ReeCampagne.etat` (refus
  « campagne terminée », arête when) ; ReeControlee.rejeter + `reads campagne_de`, ReeConstat.transmettre + `reads objet_de`,
  ReeDebris.identifier + `reads etat_decouverte` (sans eux Inconnu est indiscernable de l'état initial). cycles.py : aucun cycle.
- Demandes à l'intégrateur : aucune bloquante.
- Défauts / interprétations : masse catalogue > MASSE_MAJEURE ne classe « à risque » qu'à la 1re évaluation (sinon un
  déclassement serait défait à chaque révision) ; ReeNotification.clore rend 0 (pas Inconnu) sans notification, faute de
  ReeCampagne.etat ; « objet encore suivi après l'instant » = suivi(objet, h) ET h − instant > 72 h ; ReeDebris.notifier :
  traçage refusé ⇒ avis réduit, pas de refus ; prévision périmée et débris dangereux non neutralisé sont re-signalés à chaque
  balayage (aucun slot pour mémoriser l'alerte) ; MAX_ETATS = 200 < 300 participants possibles (Plein sur un « majeur »
  géant) ; ReeRisque.figer et ReeControlee.constater_rentree tiennent leur `when ReeCampagne.rentree` par l'ordre des appels.
- Trous laissés : aucun.
- Limites de Soma : transition vers une cible calculée refusée (E0120, écrire chaque cible) ; un `if` rendu dans un slot
  nominal doit être enveloppé (`ReeContenu(if …)`) ; les doublures sont cadrées (pas d'appel ApiAcces depuis un ApiRef
  provisoire) ; `mock` à poser juste avant l'assertion qui le consomme.
