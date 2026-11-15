# priorites — notes d'architecture
**Découpage** (9 cells, 7 machines) : `PrioVersion` (cycle d'une version, rédaction → consultation → vote → adoptée →
publiée → en vigueur → remplacée/abrogée), `PrioConsultation` et `PrioScrutin` (même id que la version : avis motivés
avec réponse d'ORBITE, bulletins, quorum 50 % et majorité 67 %), `PrioTable` (lignes critère/effet et préavis par
propulsion, éditables en rédaction seulement, figées ensuite), `PrioDerogation`, `PrioEssaim` (constellations,
évitement autonome), `PrioPreparation` (délai propre aux propulsions faibles, sans cycle), `PrioDesignation` (chaque
`qui_manoeuvre` est une décision enregistrée : version, rapprochement, TCA, instantané des entrées ; rejouable,
contestable, communiquée sous contrôle `ApiDonnees`, archivée puis purgée hors gel), `PrioEcheancier` (registre unique
des échéances, balayé ; un retard > 1 h déclare un incident : rien n'est oublié).
**Choix** : les propriétés 2 et 3 sont câblées dans `designer` (incapable = non `peut_manoeuvrer`, en anomalie,
opérateur inactif, ou préavis > temps restant ; habité seulement si seul capable) ; la table, votée, n'ordonne que le
choix entre deux capables et ne peut pas les lever. Une dérogation n'exempte jamais un objet seul capable. Pas de
changement de règles pendant un gel (`ApiOperations.changement_autorise`) : aucune coordination ne change de règles en
route. Un amendement est une nouvelle version (`base_de`, `PrioTable.reprendre`) : jamais de retour en rédaction.
Propulsion faible (lecture confirmée) : elle manœuvre si h + préavis ≤ TCA(r), sinon elle est incapable.
**Règles entre machines posées dans les cadres** : `PrioVersion consultation -> vote when PrioConsultation.close`,
`vote -> adoptee when PrioScrutin.adopte`, `vote -> rejetee when PrioScrutin.rejete`, `vote -> retiree when
PrioScrutin.annule` (même id) ; `PrioDerogation en_instruction -> octroyee when PrioVersion.en_vigueur via version_de`,
`octroyee -> caduque when PrioVersion.remplacee || PrioVersion.abrogee via version_de`.
**Règles entre sous-systèmes proposées** :
- Cible = coordination:désignation arrêtée ; exige = PrioDesignation.calculee || confirmee ; lien = `decision_de` côté coordination (r → PD-n, à créer, écrit à l'ouverture)
- Cible = PrioDerogation.octroyee ; exige = operateurs:opérateur actif ; lien = PrioDerogation.op_de
- Cible = PrioDerogation.octroyee ; exige = ref:objet en_service ; lien = PrioDerogation.objet_de
- Cible = PrioEssaim.reconnu ; exige = operateurs:opérateur actif ; lien = PrioEssaim.op_de
- Cible = PrioVersion.en_vigueur ; exige = operations:hors gel des changements ; lien = clé globale (while)
- Cible = PrioDesignation.purgee ; exige = responsabilite:aucun dossier gelé sur le rapprochement ; lien = PrioDesignation.rapprochement_de
**⚠ attendus** : bornes `.size` et `courante.size <= 1` runtime-checked (cadres des trous ; les corps
poseront `require len(...) < N`), `ensures` des types énumérés String, obligations ouvertes des `via version_de`
(écrit une fois par `demander`). Les « invariant between slots » fixent l'ordre d'écriture des corps.
**Phase 1b (contrat v2)** : façade 6/6 branchée (`decision_de` → PrioDesignation.decision_de, `balayer` →
PrioEcheancier.balayer(h, par), lot MAX_LOT) ; aucun `every`/`now` (il n'y en avait pas). Trous fermés : TCA lu
sur `r` (objets(r) = {a, b} sinon Incoherent, TCA passé → "") au lieu de l'intersection `ouverts_de` ; liste
électorale = `ApiRef.etats_participants` et destinataires de publication = `ApiRef.organisations("")` (plus fournies
par l'appelant) ; critère `masse_moindre` (ApiRef.masse) ; purge sous `sous_gel` du CJ et des OBJ +
`purge_autorisee` ; `notifier` avec r = "". Anomalie/passivation : couvertes par `peut_manoeuvrer`.
Demandes : aucune bloquante ; `ApiRef.catalogue_le(objet)` reste souhaitable (ordre d'arrivée si injecte_le = 0).
## Phase 2 (coordination + priorites)
**Règles écrites : 10, forme arête, aucune en toml.**
1. CoordDossier ouverte -> proposee | ouverte -> arbitrage ⇐ ConjRapprochement.alerte (même r) — ✓
2. CoordDossier * -> close ⇐ ConjRapprochement.resolu || passe || collision (même r) — ✓
3. CoordDecision prise -> designee ⇐ RefObjet.en_service via objet_de (désignation arrêtée ⇐ objet en service) — ✓, lien écrit par le trou prendre
4. CoordDossier arretee -> liberee ⇐ ManEvitement.libre || solde (même r ; rouverture ⇐ aucun évitement approuvé) — ✓
5. CoordSuivi execute -> verifie ⇐ ManPlan.verifie via plan_de — ✓, lien écrit par le trou suivre
6. CoordDecision levee|soldee -> purgee ⇐ RespPurges.autorisee via purge_de — ✓, lien écrit par le trou purger
7. PrioDerogation en_instruction -> recevable ⇐ OpMembres.actif via op_de — ✓ (trou demander)
8. PrioDerogation recevable -> eligible ⇐ RefObjet.en_service via objet_de — ✓ (trou demander)
9. PrioEssaim declare -> reconnu ⇐ OpMembres.actif via op_de — ✓ (trou declarer)
10. PrioDesignation archivee -> purgee ⇐ RespPurges.autorisee via purge_de — ✓ (trou purger)
**Écartées** : arrêtée ⇐ PrioDesignation.calculee|confirmee (un PD par tour, donc lien non unique ; l'arbitrage peut s'écarter des règles ; une contestation ne suspend rien) ; arrêtée ⇐ EphSuivi.a_jour (la fraîcheur est exigée à l'approbation du plan, propriété 4 ; l'exiger à la désignation bloquerait l'arbitrage) ; arrêtée ⇐ non FdvPassivation / non ObsDetections.confirmee (négations, déjà couvertes par peut_manoeuvrer) ; arbitrage ⇐ reprise d'escalade / message passerelle parti (le filet limite − 2 h est voulu, propriété 5, et le lien r → dossier ou message n'est pas unique, un par partie) ; close ⇐ aucun dossier d'escalade actif (négation) ; arrêtée ⇐ trace journalisée (pas une machine) ; PrioVersion.en_vigueur ⇐ hors gel (OpsGel n'a pas d'état global positif, tenu à l'exécution par ApiOperations.changement_autorise).
**Verify** (2 passes, dernière : VERIFY OK, 1031 ✓) : aucun ✗ ni « not explored » sur mes règles. Le seul ⚠ restant (EscVeille.couverte) appartient à escalade. Première passe : la règle ManEvitement posée sur CoordVerrou tenu -> libre visait un état initial (obligation impossible) : déplacée sur CoordDossier (règle 4).
**Structure** : états CoordDossier.liberee, CoordDecision.designee et abstention (décision « ne pas manœuvrer », levée et soldée comme en_vigueur), PrioDerogation.recevable et eligible. Slots CoordDecision.objet_de et purge_de, PrioDesignation.purge_de. Les deux `purger` prennent un paramètre `purge`. PrioDesignation.purger devient un geste « conformite » : l'échéancier signale la purge due sans l'exécuter (il n'a pas l'id de purge). Ajouts aux calls : ApiRef.etat_objet (CoordDecision.prendre, PrioDerogation.octroyer), ApiResponsabilite.purge_autorisee (CoordDecision.purger). cycles.py : aucune composante, profondeur 23.
## Phase 3 (corps)
**Remplis 88/88** (82 internes dont 2 getters ajoutés + 6 façade), 1 363 lignes de corps (sys/priorites/chair/*.cell), aucun trou laissé. `check banc_priorites.cell` : 0 erreur.
**Tests** : test_priorites.cell, 19 cells de test (301 assertions), **tous verts** ; doublures/priorites.cell (ApiAcces, ApiRef, ApiOperateurs, ApiConjonctions, ApiEvenements, ApiConformite, ApiLancements, ApiDonnees, ApiResponsabilite, ApiOperations, ApiPasserelle ; conventions lues dans les ids, en tête du fichier). Couvre les 7 machines de bout en bout, les refus Habilitation/QuatreYeux/portée/EtatInvalide, gel des changements, abrogation (Ferme), et un scénario complet (version adoptée → essaim + dérogation → désignations → contestation rejouée et confirmée → communication → archivage → amendement V2 qui remplace V1).
**Cadres corrigés (amendés, `skeleton app.cell --amend`, check app.cell 0 erreur, cycles.py : aucune composante, profondeur 23)** : getters `PrioConsultation.etat` et `PrioScrutin.etat` (sans eux, les règles d'arête `consultation -> vote when PrioConsultation.close` et `vote -> adoptee|rejetee|retiree when PrioScrutin.*` ne pouvaient pas être `require`) ajoutés aux calls de mettre_au_vote, prendre_acte, retirer ; `PrioVersion.etat` dans les calls de PrioDerogation.octroyer et caduquer (règles `via version_de`) ; `reads titre_de` (prendre_acte, retirer, abroger), `reads objet_de` (PrioDerogation.instruire), `reads echeance_de` (PrioConsultation.annuler, PrioScrutin.annuler) pour pouvoir refuser Inconnu.
**Demandes à l'intégrateur** : aucune bloquante. Approximation : la règle `archivee -> purgee when RespPurges.autorisee via purge_de` est tenue par `ApiResponsabilite.purge_autorisee(r, h)` sur le rapprochement (la façade ne juge pas un id de purge) ; un getter `ApiResponsabilite.purge_valide(purge, cible)` la rendrait exacte.
**Défauts de conception trouvés** : (1) les règles d'arête entre machines internes sans getter dans les calls (corrigé ci-dessus) ; (2) une dérogation dure ≤ 90 j et une version entre en vigueur ≥ 90 j après sa rédaction : la caducité ne touche que les dérogations octroyées tard dans le cycle de la version suivante (testé) ; (3) une échéance créée PENDANT un balayage (caducité planifiée par entrer_en_vigueur) n'est traitée qu'au balayage suivant (une minute) ; (4) l'incident de retard est déclaré une fois, au premier balayage qui le constate (dernier_balayage − échéance ≤ TOLERANCE), sans slot dédié.
**Limites de Soma rencontrées** : piège 1 confirmé aussi pour `&&` (`require !a && b` lu `!(a && b)`, deux occurrences corrigées en `(!a) && b` ; `if` et `let` ne sont pas touchés) ; types nominaux stricts aux appels (Horodatage, Objet, PrioChoix : `Horodatage(x)`, `to_string(o)`) mais pas sur les comparaisons de Map ; concaténation de listes en boucle (avertissement quadratique, listes bornées ≤ 16 ou 1 000).
## Règles de barre (COLREG, 15/11/2026)
`PrioDesignation.designer` câble, après les propriétés 2 et 3 et avant la table : règle 18 (entre faible et impulsionnelle,
l'impulsionnelle capable s'écarte, motif "manoeuvrabilite" ; une dérogation de l'impulsionnelle seule l'exempte si la faible tient),
puis seul capable, puis règle 13 (rattrapage, `ApiConjonctions.en_rattrapage`, le plus bas — altitude moyenne de
`ApiObservation.perigee/apogee` — s'écarte, motif "rattrapage"). La table ne compare plus que deux objets de même capacité :
"propulsion_faible" y est inerte ; "arrivee_recente / designe" porte « le dernier arrivé s'écarte ». Instantané à 15 champs
(+ altitude, vitesse relative, rattrapage) ; un instantané à 12 champs se rejoue sous l'ordre d'avant (propriété 6).
