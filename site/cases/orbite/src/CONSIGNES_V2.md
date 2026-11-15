# ORBITE COMMUNE — phase 1b : mise au contrat v2

Les 20 squelettes de la phase 1 sont intégrés (`app.cell` : check 0 erreur, VERIFY OK). Les
retours des 20 architectes ont produit le **contrat v2** (déjà écrit dans `commun/` et les
`sys/*/api.cell`, que tu ne modifies pas). Ta tâche : mettre TON squelette interne au contrat v2.

Binaire : `S=~/fun/soma-banc/binaires/soma-v3.2.4-cone2`. Travaille dans `~/fun/orbite`. Relis
`CONSIGNES.md` (exigences de qualité inchangées) et ta façade `sys/<toi>/api.cell` (ce qui a changé
est signalé ci-dessous ; `git` n'existe pas : compare avec ton facade.txt).

## Ce qui change pour tous

1. **Horloge centrale.** `commun/horloge.cell` : `Horloge.battre()` (every 60s) est, avec
   `PasEntree.traiter` (entrée HTTP), le SEUL lecteur du temps. Elle appelle `ApiX.balayer(h, "systeme")`
   de chaque façade. Chez toi : supprime tout `every` / `after` et tout appel à `now()` / `now_ms`
   (cadres compris) ; tes rondes et échéances deviennent des signaux appelés par ta façade
   `balayer(h, par)` — branche-la (facade.txt) sur un signal interne qui fait tout le balayage.
2. **Nouveaux signaux de TA façade** : chacun doit être branché sur un signal interne (existant ou
   nouveau) — `facade.txt` complet, une ligne par signal de ta façade, forme
   `ApiX.signal: calls Cell.sig, Cell.sig2` (seulement des noms `Cell.signal` après `calls`, pas de
   commentaire entre parenthèses ; tu peux ajouter `; moves …` après un point-virgule).
3. **Signatures changées** (les cadres ne citent que des noms : rien ne casse, mais mets à jour tes
   commentaires et la façon dont tes signaux s'en servent) :
   `ApiCoordination.designe(r, par)` · `ApiPriorites.qui_manoeuvre(h, r, a, b)` ·
   `ApiAcces.ouvrir_urgence(h, personne, r, motif, par)` · `ApiPasserelle.notifier(h, destinataire, r, sujet, texte, urgence)` ·
   `ApiResponsabilite.sous_gel(cible)` (cible : "CJ-…", "OBJ-…", évènement, plan, "J:<jour>") ·
   `ApiEphemerides.deposer(…, empreinte, position, avec_manoeuvres, par)` ·
   `ApiEvenements.declarer(h, genre, objets, r, instant, description, par, valideur)` ·
   `ApiEvenements.anomalie(h, objet, genre, description, par)`.
4. **Getters disponibles** que tu avais demandés : utilise-les pour fermer les trous de contrôle
   que tu avais contournés (liste « Pour toi » ci-dessous). Ajoute-les aux `calls` concernés.
5. **Sessions humaines** : `acces` en est seul propriétaire (`ApiAcces.ouvrir_session`,
   `authentifier`, `fermer_session`).
6. Propriété 2 renforcée : `ApiRef.peut_manoeuvrer` exclut aussi un objet en anomalie ou passivé,
   et toute perte de capacité rouvre les coordinations où l'objet est désigné.

## Règles du jeu (inchangées)

Tu ne touches que `sys/<toi>/` sauf `api.cell` ; une modification d'`entree.cell` seulement pour
ajouter/retirer tes `use`. Contrôle : `$S check app.cell` (0 erreur ; une erreur dans le fichier
d'un AUTRE agent en cours d'écriture est transitoire : réessaie une minute plus tard) — les
`entree.cell` ne se contrôlent plus seules. **Pas de `verify`** (l'intégrateur le lance).
Pas de git. Reste vers 900–1 100 lignes (un dépassement justifié est acceptable). Si le contrat v2
te manque encore quelque chose : NOTES.md, section « Phase 1b — demandes », avec la modification exacte.

Compte rendu (≤ 12 lignes, et section « Phase 1b » de NOTES.md) : signaux de façade branchés /
total, trous fermés, horloge retirée, ce qui manque encore.

## Pour toi

| Sous-système | À faire (en plus des points communs) |
|---|---|
| ref | brancher genre, etats_participants, organisations, catalogue, masse, valeur_au, attribuer, immatriculer, immatricule, changer_regime, changer_tutelle ; peut_manoeuvrer lit ApiEvenements.en_anomalie et ApiFinDeVie.passive ; propriétaire provisoire ORBITE pour un objet non attribué ; RefObjet.rouvrir_designations appelé aussi quand la propulsion passe à "aucune" (passivation déclarée par fin_de_vie via declarer_propulsion) |
| acces | ouvrir_urgence avec r (contrôle ApiConjonctions.niveau(r) == 3, ouverts), en_urgence_pour ; ouvrir_session, authentifier, fermer_session, actifs_de ; AccesHorloge et now_ms supprimés → balayer |
| operateurs | actifs, contacts (échelons 1-2-3) ; ApiRef.genre au dépôt ; changement de contrôle → ApiRef.changer_tutelle |
| ephemerides | deposer avec position → ApiObservation.ecart_a À LA RÉCEPTION (rejet si écart > seuil ; « Inconnu » d'observation : validé en qualité "faible") ; fondement, empreinte, avec_manoeuvres, depot_valide ; EphSuivi.tic supprimé → balayer ; ApiConjonctions.urgents |
| observation | ecart_a, deposer_mesure, perigee, apogee, pistes_non_correlees ; une détection confirmée appelle ApiManoeuvres.constater_non_annoncee ; une piste cataloguée a pour propriétaire ORBITE (ApiRef.orbite()) puis ApiRef.attribuer |
| conjonctions | urgents, distance, mis_a_jour, historique, clos_entre, criblage_plan, campagne_fraiche, constater_collision (sur le verdict ConjAnalyseTca), exempter (ApiFinDeVie.retrait_autorise, quatre yeux), renforcer (appelé par meteo et evenements) ; cadence des campagnes → balayer |
| coordination | designe(r, par) avec contrôle de diffusion (ApiDonnees.peut_recevoir) ; decidee, muets, decision, en_attente_de ; qui_manoeuvre(h, r, a, b) ; arbitrage pour silence après ApiEscalade.reprise ; en crise ApiOperations.arbitrage_centralise ; jamais désigner un objet que ApiRef.peut_manoeuvrer refuse ; balayer des échéances |
| priorites | qui_manoeuvre(h, r, a, b) (préavis contre ApiConjonctions.tca(r) ; « faible manœuvre si son préavis tient avant le TCA, sinon l'impulsionnelle ») ; decision_de ; refus Incoherent, Plein |
| manoeuvres | objet_de, motif_de, plans_de, plan_couvrant, constater_non_annoncee (invalide l'éphéméride, ApiConformite.constater), retirer (déjà ajouté, à vérifier) ; approbation d'un évitement : ApiCoordination.designe(r, "systeme") == objet ; désorbitation contrôlée : ApiReentrees.controlee_approuvee ; transfert vérifié → ApiRef.changer_regime ; ManVeille → balayer |
| escalade | reprise ; contacts par échelon via ApiOperateurs.contacts ; qui n'a pas répondu via ApiCoordination.muets ; décision via ApiCoordination.decidee ; veille sur ApiConjonctions.urgents ; every → balayer |
| reentrees | a_risque, fenetre, victimes_ppm, controlee_approuvee, constatee ; ApiMeteo.f107_prevu ; restitution des débris aux ApiLancements.etats_de_lancement ; notifications aux ApiRef.etats_participants |
| lancements | etats_de_lancement ; ApiRef.attribuer / immatriculer / immatricule ; COLA sur ApiConjonctions.campagne_fraiche ; retard d'immatriculation → ApiConformite.constater_etat ; ApiResponsabilite.sous_gel(cible) |
| fin_de_vie | passive, mission_capturee, plan ; ApiObservation.perigee/apogee ; ApiManoeuvres.objet_de/motif_de ; rendez-vous de retrait actif → ApiConjonctions.exempter ; passivation vérifiée → ApiRef.declarer_propulsion(objet, "aucune") (rouvre les coordinations) |
| evenements | existe, objets, genre, instant, evenement_de ; declarer avec r : une collision appelle ApiConjonctions.constater_collision et ApiResponsabilite.geler ("systeme") ; anomalie avec genre : pour chaque r de ApiConjonctions.ouverts_de(objet) où ApiCoordination.designe(r, "systeme") == objet → ApiCoordination.rouvrir ; alerte de nuage aux propriétaires du régime (ApiRef.catalogue + ApiRef.proprietaire) |
| responsabilite | sous_gel(cible), purge_autorisee, chaine ; gel automatique par ApiEvenements.existe/objets/genre/instant ; pièces au moment de la décision (ApiEphemerides.fondement/empreinte, ApiRef.valeur_au, ApiCoordination.decision, ApiPriorites.decision_de, ApiManoeuvres.plans_de, ApiConjonctions.historique) ; réclamation contre les ApiLancements.etats_de_lancement ; geler accepte "systeme" ; un dossier scellé peut recevoir un COMPLÉMENT (nouveau dossier lié) |
| donnees | ApiRef.catalogue pour la publication ; ApiConjonctions.clos_entre ; ApiResponsabilite.sous_gel(cible) avant toute purge ; balayer |
| meteo | renforce, f107_prevu ; ApiConjonctions.renforcer au lieu d'attendre qu'on sonde ; avis de tempête aux ApiOperateurs.actifs et ApiRef.etats_participants ; ApiResponsabilite.sous_gel("J:<jour>") avant archivage |
| conformite | constater_etat ; balayage des ApiOperateurs.actifs ; rapports aux ApiRef.etats_participants ; ApiPasserelle.echoue ; silencieux via ApiCoordination.muets |
| passerelle | notifier avec r (abandonne la convention « FORMAT:CJ-n ») ; echoue ; sessions humaines → ApiAcces.authentifier / ouvrir_session / fermer_session (retire PasSessions/PasJetons humains ; garde, si tu veux, des clés machine à machine) ; PasHorloge supprimé ; routes métier pour les nouveaux signaux à `par` (ApiObservation.deposer_mesure pour les capteurs, ApiConjonctions.exempter, ApiManoeuvres.constater_non_annoncee, ApiRef.attribuer/immatriculer, ApiAcces.ouvrir_session…) ; CDM avec ApiConjonctions.distance |
| operations | arbitrage_centralise ; changement_autorise sur ApiConjonctions.urgents(h, 6 h, 3) ; information de crise aux ApiRef.etats_participants ; balayer |
