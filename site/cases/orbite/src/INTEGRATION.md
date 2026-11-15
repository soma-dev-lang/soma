# ORBITE COMMUNE — bilan d'intégration de la phase 1 (14/11/2026)

## Chiffres
- 20 sous-systèmes, contrat (types, journal, 20 façades) + 20 squelettes internes écrits en parallèle par 20 agents (6 à 11 min chacun), chacun ne voyant que le contrat.
- 19676 lignes de squelette ; 210 skeletons, 159 machines à états, 1 634 signaux (tous des trous).
- `check app.cell` : 0 erreur, 185 avertissements.
- `verify app.cell` (binaire ~/fun/soma-banc/binaires/soma-v3.2.4-cone2) : **VERIFY OK**, 0 ✗, 214 s.
  Prouvées notamment : désignation unique (CoordDossier.arretee ⇐ CoordVerrou.tenu, CoordVerrou.tenu ⇐ accord|arbitrage),
  criblage avant approbation (ManPlan.crible ⇐ ManCriblage.sur), exécution dans la fenêtre, vérification après manœuvre.
- 1 règle non explorée : EscConcertation.convoquee ⇐ EscDossiers.e4 (chaîne d'appels de profondeur > 12 via la passerelle).

## Ce que l'intégration a révélé
1. Cycle d'appels entre sous-systèmes : CoordDossier.rouvrir → ApiManoeuvres.annuler → ManPlan.annuler →
   ApiCoordination.rouvrir → CoordDossier.rouvrir. Les 13 règles centrales sortaient « not explored ».
   Corrigé : nouveau geste ManPlan.retirer / ApiManoeuvres.retirer (retrait à la demande de la coordination,
   sans rappel) ; plus aucun cycle (contrôle par composantes fortement connexes du graphe d'appels).
2. Prouveur (correctif local ~/fun/soma-src/soma, product.rs, non commité) : quand la recherche complète n'est
   pas explorable, verify montre désormais ce que le CÔNE a trouvé (contre-exemple ou cause), marqué
   « possiblement fallacieux ». C'est ce qui a nommé le cycle. 1 105 tests du compilateur verts.
3. Chaîne d'appels la plus profonde : 27 niveaux (ApiPasserelle.request → … → RefObjet.nature).

## Suite
- Contrat v2 : CONTRAT_V2_A_FAIRE.md (trous de contrôle, getters demandés, horloge centrale, doublon sessions).
- Phase 2 : règles entre sous-systèmes (≈ 100 proposées dans RETOURS_PHASE1.md / sys/*/NOTES.md).
- Phase 3 : corps, scénario de bout en bout, criblage en [native].

## Contrat v2 et phase 1b (14/11/2026)
- Contrat v2 écrit par l'intégrateur depuis les 20 retours (CONTRAT_V2_A_FAIRE.md) : horloge centrale
  (commun/horloge.cell, seul lecteur du temps avec PasEntree.traiter ; ApiX.balayer(h, "systeme") dans
  les 20 façades), ~90 signaux de façade ajoutés, 9 signatures modifiées (designe(r, par), qui_manoeuvre
  avec r, ouvrir_urgence avec r, notifier avec r, sous_gel(cible), deposer avec position, declarer avec r,
  anomalie avec genre), sessions humaines chez acces seul, rôle auditeur.
- Phase 1b : les 20 architectes (reprise de leur contexte) ont mis leur squelette au contrat v2 (consignes
  CONSIGNES_V2.md, comptes rendus RETOURS_PHASE1.md « Phase 1b »).
- Résultat : 20 808 lignes, 220 skeletons, 156 machines, 1 774 signaux. check 0 erreur (176 ⚠).
  Aucun cycle d'appels (composantes fortement connexes). Profondeur maximale 23 (était 27).
  **VERIFY OK en 4 min 17, 0 ✗, 0 « not explored »** ; 73 règles entre machines prouvées, dont la
  désignation unique, le criblage avant approbation, l'exécution dans la fenêtre, la vérification après
  manœuvre et la convocation des États à l'échelon 4 (non explorée en v1).
- Squelette gelé : skeleton.lock (220 skeletons).

## Demandes restantes (non bloquantes, pour la phase 2)
designe(h, r, par) ; ApiManoeuvres.retire(plan) ; ApiAcces.personnes_de(org, r) ; ApiObservation.suivi_depuis ;
ApiConjonctions.lever_renforce, objets_urgents ; ApiEscalade.graves_de ; ApiRef.catalogue_le ; format sur
notifier ; contacts d'échelon 2 sans direction de l'opérateur ; coordination n'appelle retirer que sur un plan
declare/crible/approuve ; réentrance signaler↔coordination et constater_collision↔evenements.
Ordre imposé aux corps : reentrees/evenements écrivent constat/évènement AVANT ApiRef.changer_etat_objet.

## Phase 2 : règles entre sous-systèmes (14/11/2026)
- 10 agents (2 sous-systèmes chacun), consignes CONSIGNES_REGLES.md ; comptes rendus en fin de sys/*/NOTES.md « Phase 2 ».
- Environ 70 règles écrites (≈ 60 sur les arêtes, 10 tables toml fusionnées dans soma.toml) ; une quarantaine écartées
  avec leur raison (lien non écrit une fois, négation sur un ensemble, arête déjà prise, explosion de verify).
- Nouveaux états de passage : chaîne d'approbation d'une manœuvre crible → recoupe → coordonne → habilite →
  localise → capable → approuve (une condition par arête : criblage sûr, coordination arrêtée, opérateur actif,
  éphéméride à jour, objet en service, verrou engagé) ; ConjRapprochement.evite ; ConjCriblagePlan.fonde ;
  EphDepot.admis ; RespFautes.designation_presumee ; CoordDossier.liberee…
- **VERIFY OK en 6 min 27, 0 ✗, 0 « not explored »** : 94 règles prouvées (73 en v2), dont 21 entre
  sous-systèmes (ex. coordination ouverte ⇐ rapprochement en alerte ; rapprochement en coordination ⇐
  coordination proposée/arbitrée ; coordination close ⇐ rapprochement résolu/passé/collision ; rouverture ⇐
  évitement libre ou soldé ; plan criblé sûr ⇐ plan déclaré ; collision transmise ⇐ dossier gelé ; fin de vie
  rentrée ⇐ objet rentré) ; 149 règles `via` en obligation ouverte (lien écrit par un signal sans corps).
- 21 031 lignes, 220 skeletons, 156 machines, aucun cycle d'appels, profondeur 23. Regelé (skeleton.lock).
- Pour la phase 3 : ordre d'appel (reentrees / evenements avant ApiRef.changer_etat_objet ; changer_etat_objet doit
  transmettre la référence de la cause) ; getters demandés : ApiReentrees.campagne_de, ApiFinDeVie.mission_de,
  ApiEvenements.confirme.
