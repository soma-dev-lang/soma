# meteo — notes d'architecte (squelette)
## Choix de conception
- 11 cells, 8 fichiers : MetSource ; MetMesure + MetVeille ; MetPrevision ; MetTempete ;
  MetRenforce + MetAvis ; MetBareme ; MetIncertitude + MetRonde ; MetJournee.
- Chaîne de confiance en règles : mesure validée ⇐ source active (`via source_de`) ; prévision
  publiée ⇐ centre actif (`via source_prev`) ; alerte en cours ⇐ mesure validée (`via
  confirmee_par`) ; renforcement levé ⇐ alerte terminée, abandonné ⇐ alerte annulée (même id) ;
  mesure archivée ⇐ journée scellée (`via jour_de`).
- Une seule alerte ouverte à la fois (`ouverte.size <= 1`) : une tempête est planétaire.
- Rien d'absent ne bloque (propriété 5) : flux muet = `MetVeille.silence`, arrêt périodique en
  retard = `MetIncertitude.perime`, source muette = panne par balayage (relais par priorité),
  destinataire muet = `MetAvis.sans_reponse` + manquement ; le facteur passe alors en précaution
  (bande "tempete") et un incident est déclaré.
- Le facteur servi par la façade est ARRÊTÉ avec son fondement et journalisé par paliers :
  `MetJournee.conditions_a` restitue pour tout instant passé mesure, alerte, barème et facteur
  effectivement servis (propriété 6). Rien n'est purgé. Données météo : Diffusion "public".
- Barème versionné à quatre yeux, jamais changé pendant une alerte (Conflit) ; le renforcement
  fige ses facteurs. Valeur hors échelle physique = refus HorsBornes ; valeur plausible mais
  douteuse = conservée "rejetee" avec motif (preuve).

## Avertissements check / verify
- check : 7 « invariant between slots … ?? 0 » ; gauche absent = 0, droit absent = FIN_DES_TEMPS
  (an 3000) : l'invariant tient quel que soit l'ordre d'écriture. verify : ⚠ runtime-checked
  des bornes `size` (corps : `require len(..) < MAX`) et des `ensures` sur types String.
## Règles entre sous-systèmes proposées
- Cible = MetSource.active ; exige = ref:organisation_existe(fournisseur) ; lien = MetSource.fournisseur
- Cible = MetMesure.archivee ; exige = responsabilite:aucun gel couvrant le jour ; lien = MetMesure.jour_de
- Cible = MetBareme.en_vigueur ; exige = operations:changement_autorise ; lien = id de version
## Phase 1b (contrat v2) — aucun every/after/now n'existait dans meteo
- Façade : 6/6 signaux branchés (facade.txt). balayer → MetRonde.balayer (dans incertitude.cell,
  app.cell n'important que mes 8 fichiers), qui enchaîne sources, veille, prévisions échues,
  alertes, prolongation des épisodes, facteurs, avis, journées ; plus d'« ordonnanceur ».
  renforce → MetRenforce.renforce ; f107_prevu → MetPrevision.f107_moyen (nouveau).
- Trous fermés : criblage renforcé demandé à ApiConjonctions.renforcer (épisodes de 24 h par
  régime, prolongés par la ronde : MetRenforce.prolonger) ; avis aux ApiOperateurs.actifs et aux
  autorités de ApiRef.etats_participants (org_de_etat) ; archivage refusé sous
  ApiResponsabilite.sous_gel("J:<jour>") ; notifier avec r = "".
## Phase 1b — demandes
- ApiConjonctions.lever_renforce(episode, par) : un épisode court sinon jusqu'à 24 h après la levée.

## Phase 2 — règles entre sous-systèmes (conjonctions + meteo)
- Écrites (7) : arête sauf mention.
  1. ConjRapprochement.en_coordination ⇐ CoordDossier.proposee || arbitrage (même id r ; `ouverte` n'est que transitoire dans ApiCoordination.ouvrir) — engager.
  2. ConjRapprochement résolu par manœuvre ⇐ ManPlan.verifie via plan_de (nouveau slot, écrit une fois par resoudre) : nouvel état transitoire `evite` (en_coordination -> evite when … -> resolu) ; « manoeuvre_verifiee » seulement depuis en_coordination.
  3. ConjCriblagePlan.sur ⇐ ManPlan.declare (même id plan) : nouvel état transitoire `fonde` (demande -> fonde when ConjCampagne.terminee via campagne_de ; fonde -> sur when ManPlan.declare ; fonde -> a_risque). La forme toml (while) sortait ✗ (trou cribler sans la clause) : abandonnée.
  4. ConjAnalyseTca.confirmee ⇐ EvtEvenements.signale || confirme via evenement_de (2 arêtes, confirmer).
  5. ConjMessage.escalade ⇐ EscVeille.surveillee || couverte via rapprochement_de (escalader ; la veille par rapprochement est armée par EscDossiers.ouvrir).
  6. MetSource.active (declaree -> active, qualifier) ⇐ RefOrg.inscrite || active via fournisseur ; calls + ApiRef.organisation_existe.
  7. MetMesure.archivee ⇐ RespGels.libre via gel_de (nouveau slot "J:<jour>", écrit par recevoir) — TOML, sys/meteo/regles.toml (arête déjà prise par MetJournee.scellee).
- Écartées : resolu ⇐ coordination arretee|close (circulaire avec CoordDossier.close ⇐ conj resolu, et pc retombée doit résoudre avant l'arrêt) ; resolu ⇐ ReeCampagne.rentree (aucun motif « rentrée », lien ambigu entre deux objets) ; MetBareme.en_vigueur ⇐ changement autorisé (fait global ApiOperations.changement_autorise, aucune machine/lien par version) ; ConjRenforcement ⇐ MetRenforce non levé (épisode multi-causes tempête/nuage/manuel, pas de lien météo) ; MetSource retablir/reprendre_service non conditionnés (arêtes de retour).
- verify (2 passes, copie avec regles.toml fusionné) : 0 ✗ et 0 « not explored » sur mes règles ; ⚠ seulement « open obligation … written by the hole » (plan_de, evenement_de, rapprochement_de, fournisseur, gel_de). Seul ✗ global : EscVeille.couverte ⇐ CoordDossier.arretee (escalade, pas à moi). check 0 erreur ; cycles.py : aucune composante, profondeur max 23.
- Structure : états `evite` (ConjRapprochement), `fonde` (ConjCriblagePlan) ; slots ConjRapprochement.plan_de, MetMesure.gel_de ; refus EtatInvalide ajouté à cribler.

## Phase 3 — corps
- Signaux remplis : 94/94 (11 cells internes + ApiMeteo), 1 638 lignes de corps (sys/meteo/chair/, 9 fichiers) ; aucun trou laissé.
- Tests : test_meteo.cell, 13 tests / 401 assertions, tous verts (cycles de vie des 11 machines, refus Habilitation / QuatreYeux / portée capteur-fournisseur / états, scénario complet prévision G4 → alerte → renforcement + avis → confirmation → extension G5 → déclin → calme 6 h → clôture/levée → preuve conditions_a ; annulation, désaveu, ronde). Doublures : doublures/meteo.cell (ApiAcces, ApiRef, ApiOperateurs, ApiPasserelle, ApiConjonctions, ApiOperations, ApiConformite, ApiResponsabilite).
- Cadres corrigés (amendés, 2 `--amend`, check app.cell OK, cycles.py sans composante) :
  MetSource : slots maintenance_debut, en_service_le, panne_le (sans eux : reprise « après le début de la maintenance » et rétablissement « postérieur à la panne » invérifiables, et une source rétablie retombait en panne de silence à la minute suivante) ; reads fournisseur sur etalonner, declarer_panne, annoncer_maintenance, reprendre_service, retirer, mettre_en_ecart, noter_reception (Inconnu + portée capteur) ; calls centres_actifs sur annoncer_maintenance.
  MetMesure.archiver calls MetJournee.scellee (règle d'arête). MetPrevision.retirer reads source_prev ; f107_moyen reads courante_de (« centres actifs »). MetTempete.desavouer reads emise_le ; evaluer calls MetMesure.valeur_de (émettre sur kp ≥ 7). MetRenforce : arête `declenche -> leve` (levée d'une alerte terminée dont aucune notification n'a pu partir — sinon renforce() restait vrai à jamais) ; lever/abandonner reads declenche_le. MetAvis.suivre reads destinataire. MetBareme.fixer/abandonner reads redige_par. MetJournee.a_clore reads rectifications (énumérer les journées en rectification).
- Demandes à l'intégrateur : aucune.
- Choix : id d'alerte émise par la ronde "T-<h>" ; dernier saut rejeté gardé dans `derniere["saut_<g>"]` (confirmation par une autre source à ±10 dixièmes sous 3 h) ; rang notifié gardé dans `envoyes["<alerte>#rang"]` ; une mesure d'époque future n'enregistre pas d'époque (invariant epoque ≤ recue_le + 5 min) ; a_clore regarde la veille et les 3 jours d'avant ; rejets en série des prévisions comptés en relisant les 5 derniers dépôts du centre.
- Défauts de conception restants : facteur_a ne restitue pas la précaution servie pendant un palier « perime » (constater_retard n'écrit pas paliers) ; une nouvelle prévision ne met pas à jour l'alerte ouverte (evaluer n'a que des reads) ; une prévision échue sans mesure reste publiée (verifier refuse Inconnu à chaque ronde, echues trié par fin décroissante pour ne pas bloquer) ; MetRonde ne compte pas les changements d'état intra-alerte (pas de lecture de la machine tempete).
- Limites Soma : transition() à cible ou source calculée refusée par check (branches écrites en toutes lettres) ; une locale nommée comme un signal d'une autre cell (`connu`) est lue comme un appel ; `e.value` des entries non typé.
