# ORBITE COMMUNE — intégration de bout en bout (14/11/2026)

Test : `test_bout_en_bout.cell` (racine, `use prod`, AUCUNE doublure). `$S test test_bout_en_bout.cell` :
**9 cellules de test, 182 assertions, toutes vertes** (≈ 14 s). `$S check prod.cell` : 0 erreur.
`python3 outils/cycles.py` : aucune composante (profondeur maximale 23, inchangée). Les 20 suites unitaires
restent vertes (3 adaptées — donnees, manoeuvres et sa doublure, escalade — voir écarts). Binaire : `~/fun/soma-banc/binaires/soma-v3.2.4-cone2`.

## Scénario couvert (vrais corps ; janvier 2027, jours 20819 = 01/01)

| Test | Étapes |
|---|---|
| E1Amorcage | fondation par "systeme" (`RefOrg.amorcer` ALD + ORBITE, `AccesPersonnes.amorcer` dir1) ; personnel d'ORBITE ; BRV et CAZ déclarés, candidats, admis (quatre yeux) ; autorités ; personnes des États, des opérateurs ORG-OPA / ORG-OPB et du fournisseur ORG-CAPT, rattachements confirmés ; accord de partage ADP-1 (consultation 30 j, avis des 3 États, adoption à quatre yeux, vigueur au balayage du 30/11), adhésions signées et contresignées ; règles PRIO-2027 (table, consultation, vote, publication, vigueur le 01/01 au balayage) ; agrément du fournisseur et 3 capteurs leo étalonnés ; canaux webhook vérifiés (défi) ; licences confirmées par l'État, dossiers acceptés, engagements, astreintes 04→18/01, exercice de joignabilité acquitté et accusé, chef de mission confirmé, activation ; points focaux confirmés (échelon 3) |
| E2Lancement | préavis L-2027-1 (OPA, ALD + BRV État de lancement), zones de retombée, campagne leo fraîche, COLA demandé et conclu, avis favorable à quatre yeux, manifeste (A1, A2 d'OPA ; B1, B2 d'OPB ; étage E1 sans propulsion) ; tir, injection, séparations, catalogage OBJ-1..5 |
| E3Observation | seuils d'éphémérides amorcés (EPR-0) ; éphémérides validées ; 6 mesures de 2 capteurs corrélées, orbite déterminée et validée par objet ; identification, immatriculation (registre 1975), mise en service ; identification close |
| E4Criblage | noyau natif `ConjCampagne.criblage` sur le catalogue (3 paires trouvées, calculées à la main : 150 m, 120 m, 100 m) ; signalement niveau 3 → coordination « proposee » (proposition par défaut A1, masse moindre) |
| E5BrancheA | OPA et OPB acceptent → désignation arrêtée A1 ; éphéméride avec poussée, plan PL-A1 déclaré → criblé sûr → approuvé (chaîne complète) ; propriété 1 : second plan sur le même rapprochement (même objet ou l'autre) refusé Conflit ; exécution confirmée ; détection d'écart par l'observation, expliquée par le plan ; nouvelle éphéméride ; vérification conforme → rapprochement résolu ; coordination close au balayage ; 24 h d'horloge sans erreur |
| E6Proprietes2Et4 | propriété 2 : étage E1 (plus léger, sans propulsion) jamais désigné, A2 proposé, plan sur E1 refusé NonEligible ; propriété 4 : éphéméride périmée → Perime ; catalogue non criblé depuis 8 h → plan rejeté ; opérateur suspendu (conformite + direction) → Suspendu ; rétabli → approuvé |
| E7BrancheB | OPB désigné (B2) muet : horloge (20 balayages, 1 h 30 → 16 h le lendemain, sans erreur) : escalade e3 → e4, reprise par ORBITE et arbitrage ensemble à TCA − 8 h ; arbitrage refusé sans quatre yeux / sans motif / mauvais rôle, accepté (ana1 + sup1) : A2 manœuvre ; manquement « reponse_tardive » g3 d'OPB chez conformite |
| E8BrancheC | propriété 6 : OPA restreint A2 aux « parties » ; la désignation est lue par OPA, l'État d'OPA, ORBITE, refusée (Confidentiel) à OPB, CAZ, au fournisseur ; collision déclarée à quatre yeux après le TCA → rapprochement « collision », dossier de preuve gelé (rapprochement, objet), décision intacte, chaîne de preuve de 8 pièces (coordination, rapprochement, priorités, météo, éphémérides avec empreinte, fiches d'objet à l'instant de la décision) ; 24 h d'horloge sans erreur |
| E9Rentree | prévision de rentrée de E1 (à risque), États notifiés d'office (ALD) et par survol (CAZ), accusés (habilitation de l'État vérifiée), révision imminente, constat à quatre yeux, objet « rentre » ; 24 h d'horloge puis `Horloge.battre` (now_ms simulé) sans balayage en erreur |

## Écarts d'intégration trouvés et corrigés (corps)

1. **Un fournisseur de capteurs ne peut avoir aucun administrateur (acces ↔ donnees, observation, passerelle)** —
   symptôme : l'agrément du fournisseur ORG-CAPT est impossible (`ObsFournisseurs.demander` exige
   `ApiDonnees.adherent`, et `DonAdhesions.signer` exige l'"administrateur_operateur" de l'organisation de genre
   "capteur") ; or `AccesPortees.compatible` n'admettait chez un fournisseur que le rôle "capteur" (HorsBornes).
   `PasCanaux.declarer` et `ObsFournisseurs.demander` attendent aussi cet administrateur. Doublures : agit_pour
   vrai sur le préfixe de l'id. Correction (acces, seul en désaccord avec trois sous-systèmes) :
   `sys/acces/chair/roles.cell` (`compatible` : "capteur" ou "administrateur_operateur" chez un genre "capteur") ;
   commentaires de `AccesPortees.compatible` (`sys/acces/roles.cell`) et `AccesPersonnes.enregistrer`
   (`sys/acces/personnes.cell`), `skeleton --amend`.
2. **Empreinte d'éphéméride déposée par l'API (passerelle ↔ ephemerides)** — symptôme : toute éphéméride OEM
   déposée par `POST` à la passerelle serait rejetée « format ». Cause : passerelle transmet `sha256(contenu)`
   (64 hex, type `PasEmpreinte`), ephemerides exige la forme documentée dans son squelette
   `"sha256:" + 64 hex` (`EphControle.examiner`, slot `EphDepot.empreinte_de`). Doublures : deposer acceptait tout.
   Correction : `sys/passerelle/chair/metier.cell` (`_deposer` : `"sha256:" + e` ; l'id de dépôt reste
   `OEM-<16 hex>`). `test_passerelle.cell` : 370/370 verts.
3. **Nature des traces de communication (donnees ↔ 12 sous-systèmes)** — symptôme : l'ouverture de la
   coordination du premier rapprochement de niveau 3 échouait (Incoherent « nature de communication hors liste :
   arbitrage », avalé par le `try` de `ConjRapprochement.engager` : rapprochement resté « alerte », aucune
   coordination). Cause : la façade `ApiDonnees.tracer(…, nature: String)` ne fixe aucune liste ; ses appelants
   passent du texte libre ("arbitrage", "fil", "fiche", "fondement(s)", "CDM", "mesure", "orbite", "cola",
   "lecture API", "avis_manoeuvre", "dossier_manoeuvre", "decision_priorite:…", "evenements:…", "avis de
   rentrée …", "post_mortem", "dossier de fin de vie"…), mais le corps interne `DonTraces.tracer` n'acceptait que
   les 8 valeurs de `DonNatureComm` : presque toute communication tracée (donc tout envoi de données qui trace
   avant d'envoyer) levait. Doublures : tracer acceptait tout. Correction (le contrat de façade fait foi) :
   `sys/donnees/chair/traces.cell` (nature libre, non vide, ≤ 200 caractères ; Incoherent sinon) ; commentaires
   de `DonTraces.tracer` et de `DonNatureComm` (`sys/donnees/traces.cell`, valeurs conseillées), `--amend` ;
   `test_donnees.cell` : le refus Incoherent porte sur une nature vide (308/308 verts).
4. **Vérification d'une manœuvre annoncée jamais « conforme » (manoeuvres ↔ observation)** — symptôme : un plan
   d'évitement exécuté et vu par l'observation finit en « ecart » (anomalie) : le rapprochement ne peut être résolu
   par `manoeuvre_verifiee`. Cause : `ManVerification.verifier` exigeait `ApiObservation.manoeuvre_detectee`
   VRAI ; or la façade la définit comme une manœuvre NON annoncée (`ObsDetections.detectee_depuis` : détection
   confirmée ou signalée, pas expliquée) et la poussée d'un plan approuvé est « expliquée » par
   `ApiManoeuvres.plan_couvrant` dès sa confirmation. Doublure : manoeuvre_detectee vrai sauf « NONVU ».
   Correction : `sys/manoeuvres/chair/execution.cell` (conforme ⇔ nouvelle éphéméride post-poussée validée,
   orbite suivie, écart ≤ ECART_MAX_M et AUCUNE manœuvre non annoncée depuis reel − 1 h) ; commentaires de
   `ManVerification.verifier` et des arêtes (`sys/manoeuvres/execution.cell`), `--amend` ; doublure
   `doublures/manoeuvres.cell` (manoeuvre_detectee vrai pour « INEXPL » seulement) et `test_manoeuvres.cell`
   (FenetreEchue : OBJ-INEXPL2) — 231/231 verts. Reste : demande de façade F1 (ci-dessous).
5. **Reprise par ORBITE trop tardive pour arbitrer (escalade ↔ coordination ↔ priorites)** — symptôme : dans la
   branche « opérateur muet », l'escalade prononce la reprise à TCA − 6 h ; l'arbitrage désignant l'objet de
   l'opérateur joignable est alors refusé (Delai : `CoordVerrou.poser` exige h + préavis ≤ TCA, préavis
   impulsionnel 6 h) — la reprise censée rendre la main à ORBITE (propriété 5) tombe exactement à la limite
   d'action, sans aucun temps pour arbitrer. Cause : `EscBareme.echeance_finale` = TCA − max(6 h, préavis), alors
   que la coordination passe en arbitrage à limite − MARGE_ARBITRAGE (TCA − préavis − 2 h) et que le commentaire de
   MARGE_REPRISE_MS dit « marge laissée à l'arbitrage ET à la manœuvre ». Doublures : préavis fixes, pas de
   verrou. Correction : `sys/escalade/chair/bareme.cell` (TCA − max(MARGE_REPRISE_MS, préavis +
   MARGE_ARBITRAGE_MS)) ; `sys/escalade/bareme.cell` (const MARGE_ARBITRAGE_MS = 2 h, commentaire de
   `echeance_finale`), `--amend` ; `test_escalade.cell` (valeur attendue de BaremeCalculs) — 270/270 verts.
   Reprise et arbitrage s'ouvrent désormais ensemble, 2 h avant la limite d'action.
6. **La collision effaçait la décision qu'elle devait geler (evenements ↔ ref ↔ coordination ↔ responsabilite)** —
   symptôme : après `ApiEvenements.declarer("collision", …, r)`, la coordination de `r` était rouverte en
   « arbitrage », sa décision effacée (`ApiCoordination.decision(r) == ""`), et le dossier de preuve gelait une
   pièce de coordination « absent=aucune_decision » : `ApiResponsabilite.chaine(décision)` → Inconnu (propriété 6
   rompue). Cause : `EvtEvenements.confirmer` passait d'abord les objets en « fragmente »
   (`ApiRef.changer_etat_objet` → `RefObjet.rouvrir_designations` → `ApiCoordination.rouvrir` sur le rapprochement
   encore ouvert), et seulement ensuite gelait les preuves et faisait constater la collision. Doublures : ref sans
   rouverture. Correction : `sys/evenements/chair/evenements.cell` (pour une collision : `ApiResponsabilite.geler`
   puis `ApiConjonctions.constater_collision` AVANT la fragmentation des objets ; l'ordre imposé « évènement
   confirmé avant changer_etat_objet » reste tenu). `test_evenements.cell` : 275/275 verts.
7. **Défauts de messages vers ORBITE ou un fournisseur de capteurs réessayés sans fin (passerelle ↔ conformite)** —
   symptôme : chaque battement journalisait « signalement impossible (Inconnu), réessai » pour tous les messages
   adressés à ORG-ORBITE (reprise d'escalade, avis internes…) restés en défaut ; des centaines de lignes par jour
   (le journal est borné à 10 M lignes). Cause : `PasMessages._suivre_defaut` envoyait tout défaut hors État à
   `ApiConformite.constater`, qui ne connaît que les opérateurs (Inconnu pour "orbite" et "capteur"). Correction :
   `sys/passerelle/chair/messages.cell` (destinataire ni opérateur ni État : « signale », défaut seulement
   journalisé) ; commentaire de `PasMessages.traiter_file` (`sys/passerelle/messages.cell`), `--amend`.
8. **Alertes de conjonction sans accusé remises à l'escalade sans fin (conjonctions ↔ escalade)** — symptôme : à
   chaque ronde, « ronde : Inconnu / NonEligible / EtatInvalide sur MC-n » pour les mêmes messages. Cause :
   `ConjMessage.escalader` remettait à `ApiEscalade.escalader` toute alerte sans accusé ; escalade refuse
   (définitivement) un point focal d'État (Inconnu : il ne possède aucun objet), une partie qui a répondu à la
   coordination (NonEligible) et un dossier déjà en e4/reprise (monter refusé) ; le message restait
   « sans_reponse » et la ronde recommençait. Correction : `sys/conjonctions/chair/messages.cell` (dossier déjà
   ouvert : escalade à l'échelon courant, `ApiEscalade.echelon` ; refus Inconnu/NonEligible/Ferme sans dossier :
   `sans_reponse -> caduc`, cause notée, renvoie 0) ; cadre de `ConjMessage.escalader` (`sys/conjonctions/messages.cell` :
   + writes cause_de, + moves sans_reponse -> caduc, + calls ApiEscalade.echelon), `--amend`. 275/275 verts.
9. **Le réseau de capteurs saturait en une journée (observation ↔ ephemerides, conjonctions, lancements…)** —
   symptôme : l'analyse après TCA ne pouvait plus demander de suivi (« Plein : régime leo saturé ou aveugle ») ;
   ronde « Plein sur CJ-n ». Cause : 15 appelants de `ApiObservation.demander_suivi` redemandent le même suivi à
   chaque balayage (ex. `EphSuivi.balayer` toutes les heures, priorité 3, 24 h, par objet périmé en rapprochement) et
   `ObsTaches.demander` créait chaque fois une tâche (50 par capteur). Correction : `sys/observation/chair/taches.cell`
   (une tâche active du même objet, de priorité ≥, sert la demande : prolongée, son id rendu) ; cadre et
   commentaire de `ObsTaches.demander` (`sys/observation/taches.cell` : + reads tache), `--amend`. 244/244 verts.

## Écart constaté, non corrigé (demande)

- **E1. Gel à la reprise d'escalade impossible (escalade ↔ responsabilite)** : `EscDossiers.reprendre` appelle
  `ApiResponsabilite.geler(h, <dossier ESC-n>, …)` (« évènement = dossier ») mais `RespDossiers.ouvrir` n'accepte
  qu'un évènement d'`ApiEvenements` (Inconnu, avalé par `try`, journal « gel de ESC-n refuse : Inconnu ») : aucune
  preuve n'est gelée quand ORBITE reprend la main. Il faut décider ce qu'on gèle (le rapprochement) : façade
  `ApiResponsabilite.geler` acceptant un rapprochement "CJ-…" (instant = TCA, objets = ApiConjonctions.objets),
  ou un signal `ApiResponsabilite.geler_rapprochement(h, r, motif, par)` ; escalade passerait alors `r`.

## Exigences réelles masquées par les doublures (monde du test, pas de corps fautif)

- Les satellites catalogués restent « catalogue » : sans mise en service par le dynamicien
  (`ApiRef.changer_etat_objet(…, "en_service")`), `peut_manoeuvrer` est faux et toute coordination part en
  arbitrage « sans_designation ».
- Le criblage d'un plan exige une éphéméride déposée AVEC la poussée planifiée (`avec_manoeuvres` vrai), fraîche
  (8 h en leo) — sinon le plan est rejeté « criblage_impossible ».
- Aucun dépôt d'éphéméride sans version de seuils (`EphReglage.amorcer` par un exploitant) ; empreinte au format
  `"sha256:" + 64 hex`.
- Un fournisseur de capteurs doit adhérer à l'accord de partage avant l'agrément ; une campagne de criblage du
  régime doit dater de moins de 8 h pour le COLA d'un lancement et pour tout criblage de plan.
- Les capteurs muets plus de 6 h passent « en_panne » au balayage d'observation : dans un monde simulé sans flux
  de mesures continu, la couverture devient aveugle (sans conséquence sur les tests, à savoir pour la suite).
- Tout `notifier` fait un premier `http_post` immédiat : le test mocke `http_post` pour les messages qu'il faut
  acquitter (exercice de joignabilité) ; les autres échouent (domaines `.invalid`) et suivent le circuit de défaut.
- Rien n'orchestre une campagne de bout en bout : `ConjCampagne.terminer` (comptes) et `ApiConjonctions.signaler`
  sont appelés par le moteur externe (ici le test, à partir du noyau natif `ConjCampagne.criblage`) ; de même
  `LancCola.conclure` (résultat du moteur COLA).

## Demandes de façade (non faites : api.cell)

- **F1. `ApiObservation.manoeuvre_vue(objet, depuis) -> Bool`** (détection confirmée, expliquée OU signalée) :
  `ManVerification.verifier` (écart 4 : aujourd'hui conforme par absence de manœuvre inexpliquée, sans preuve
  positive de la poussée), `ManPlan.constater_echeance` (« poussée faite mais jamais confirmée » : la poussée d'un
  plan approuvé est expliquée, donc invisible pour `manoeuvre_detectee` → le plan est annulé alors qu'il a été
  exécuté) et `EvtCollisions` (désigné ayant manœuvré) veulent « manœuvre vue », pas « non annoncée ».
- **F2. `ApiResponsabilite.geler`** sur un rapprochement (écart non corrigé E1).
- **F3. Refus QuatreYeux** : `ApiEvenements.declarer` lève Habilitation (pas QuatreYeux) quand le valideur n'est
  pas superviseur (`EvtEvenements.confirmer`), contrairement à son commentaire de façade et à `arbitrer` ;
  aligner l'un ou l'autre.
- **F4. Natures de manquement** : 30 natures libres passées à `ApiConformite.constater` (« reprise_sans_decision »,
  « silence_escalade », « escalade_sans_contact »…) deviennent « autre » (détail conservé) : fixer la liste
  dans le commentaire de façade ou élargir `ConfNature`.

## Non couvert

Propriété 3 (objet habité), fragmentation et nuage de débris (catalogage des fragments), anomalie d'objet et
perte de propulsion d'un désigné (rouverture), fin de vie / retrait actif, réclamations entre États, dérogations
de priorité, cession d'un objet, météo et tempête (criblage renforcé), mode dégradé / crise / bascule
(operations), passerelle HTTP de bout en bout (sessions, jetons, routes), rentrée contrôlée et débris retrouvés.

## Rejeu Aeolus (15/11/2026)

Test : `test_aeolus.cell` (racine, `use prod`, AUCUNE doublure) rejoue l'incident Aeolus / Starlink 44 de
septembre 2019 (faits publics seulement ; tout le reste est simulé, voir l'en-tête du test) : **5 cellules,
95 assertions, toutes vertes** (`$S test test_aeolus.cell`, binaire `soma-v3.2.4-cone4`). Chronologie écrite :
`AEOLUS_TRACE.md`. Après les corrections : `test_bout_en_bout.cell` 182/182, `test_coordination.cell` 227/227,
`test_escalade.cell` 270/270, `test_passerelle.cell` 370/370, `test_manoeuvres.cell` 231/231 ; `check prod.cell`
sans erreur ; `outils/cycles.py` : aucune composante, profondeur 23.

**Choix du rejeu, pas un écart.** Avec la table de seuils par défaut (attention 1e-5), la pc de 1/50 000 du 28/08
donne un niveau 2, et tout niveau ≥ 2 ouvre une coordination (`ApiConjonctions.signaler` → `engager`) : ORBITE
aurait ouvert la coordination dès le 28/08. Le monde de 2019 adopte une table SEU-1 alignée sur le seul seuil
public (1/10 000 comme seuil d'attention et d'urgence) ; A2 l'affirme des deux côtés.

Écarts trouvés et corrigés :

10. **Reprise immédiate dès qu'un objet à propulsion faible (ou sans propulsion) est en cause (escalade ↔
    priorites ↔ coordination)** — symptôme : l'escalade contre SpaceX s'ouvrait à l'échelon 4 et passait
    aussitôt en reprise (échéance finale = TCA − 74 h, avant même l'alerte de niveau 3 à TCA − 70 h). Cause :
    `EscBareme.echeance_finale` prenait le PLUS GRAND préavis des objets en cause (faible 72 h ; « aucune »
    ≈ 100 ans : reprise immédiate pour tout rapprochement avec un débris ou un étage), alors que la limite
    d'action de la coordination est celle de l'objet proposé. Correction : `sys/escalade/chair/bareme.cell`
    (plus PETIT préavis parmi les objets capables, propulsion ≠ "aucune" ; MARGE_REPRISE_MS si aucun) ;
    commentaire de `EscBareme.echeance_finale` (`sys/escalade/bareme.cell`), `--amend` ; `test_escalade.cell`
    (BaremeCalculs : TCA − 6 h pour FAIB face à B). Rejeu : échéance finale 02/09 4 h = TCA − 8 h.
11. **Chaque message non acquitté faisait monter l'escalade d'un échelon (passerelle ↔ escalade)** — symptôme :
    échelons 1 → 4 en 30 min, le barème (30 min, 30 min, 15 % du temps restant) contourné. Cause :
    `PasMessages._suivre_defaut` appelait `ApiEscalade.escalader` (= monter si un dossier existe) pour CHAQUE
    message de r en défaut — y compris les relances de l'escalade elle-même ; après la reprise, un défaut de plus
    rouvrait un dossier, aussitôt à l'échelon 4 et en reprise (double compte des escalades graves). Correction :
    `sys/passerelle/chair/messages.cell` (dossier actif pour (r, dest) ou reprise sur r : le défaut est versé à
    l'escalade sans la faire monter) ; cadre et commentaire de `PasMessages.traiter_file`
    (`sys/passerelle/messages.cell` : + calls ApiEscalade.echelon, ApiEscalade.reprise), `--amend` ; doublure
    `doublures/passerelle.cell` (echelon 0, reprise faux). `test_bout_en_bout.cell` E7 : la trace attendue
    était celle du défaut ; désormais 2 (2 h) → 3 (2 h 30) → 4 (8 h 30) → reprise (16 h le lendemain).
12. **Une poussée une demi-orbite avant le TCA était interdite (manoeuvres ↔ priorites)** — symptôme : le plan
    d'AEOLUS (poussée à TCA − 45 min) refusé Delai « fenêtre trop tardive ». Cause : `ManPlan.declarer` exigeait
    début de fenêtre + `ApiPriorites.preavis` ≤ TCA, alors que la façade des priorités définit le préavis comme
    le délai minimal entre la DÉCISION et le TCA (c'est l'usage de coordination : verrou et limite d'action) :
    avec 6 h de préavis, aucune poussée tardive possible, et 2 h seulement entre l'arbitrage et la dernière
    poussée permise. Correction : `sys/manoeuvres/chair/plans.cell` (h + préavis ≤ TCA : plan déclaré au plus
    tard à la limite d'action ; début < TCA) ; commentaire de `ManPlan.declarer` (`sys/manoeuvres/plans.cell`),
    `--amend`. `test_manoeuvres.cell` inchangé, 231/231.
13. **Prise en charge « dépassée » malgré un arbitrage rendu à temps (escalade ↔ coordination)** — symptôme :
    arbitrage à 4 h 05 (limite de prise en charge 4 h 15), et au battement de 4 h 30 « prise en charge de ESC-1
    dépassée : alerte de direction », incident de gravité 3 et cellule de crise. Cause : `EscPriseEnCharge.suivre`
    appelait `depasser` avant `trancher` (qui constate la décision arrêtée, `EscVeille.couverte`). Correction :
    `sys/escalade/chair/prise_en_charge.cell` (trancher d'abord). Corps seul.
14. **Manquements d'État en boucle (passerelle ↔ conformite)** — symptôme : ≈ 1 650 manquements d'État en trois
    jours d'horloge. Cause : la notification d'un manquement à un État sans canal part en défaut, et le défaut
    est signalé à la conformité comme un nouveau manquement, notifié à son tour, etc. Correction :
    `sys/passerelle/chair/messages.cell` (sujet « manquement … » ou « contradictoire … » : défaut seulement
    journalisé) ; commentaire de `PasMessages.traiter_file`, `--amend`.

Constaté, non corrigé :

- **E1** (gel à la reprise) toujours là : « gel de ESC-1 refuse : Inconnu » (demande F2).
- **Avis de manœuvre non accusé après la décision** : `ManAvis.relever` escalade (voulu : « l'autre opérateur
  doit savoir que le désigné manœuvre ») ; le dossier ESC-2 entre à l'échelon 4, en reprise, clos à la décision,
  mais compte comme une seconde « escalade grave » pour le même silence (statistiques, démarche DIP-2). À trancher :
  ne pas compter un dossier ouvert après la reprise sur le même r.
- Bruit du monde simulé, sans corps fautif : rappel horaire « manquement grave … en souffrance » tant que le
  contradictoire n'est pas ouvert ; manquements quotidiens de fin de vie (objets sans plan de fin de vie) ;
  incidents météo (aucune source Kp/F10.7, facteur de tempête 300 % appliqué par défaut) ; États et ORBITE sans
  canal (leurs messages partent en défaut).

### Rejeu Aeolus avec l'accord tacite (binaire `soma-v3.2.4`)

La coordination arrête désormais une proposition acceptée par le propriétaire du proposé quand les autres
parties sont seulement muettes (`CoordDossier.constater_silence`, « accord tacite »). Rejeu : désignation
d'AEOLUS arrêtée le 30/08 à 15 h (TCA − 69 h) par accord, plan approuvé à 15 h 20, plus de reprise ni
d'arbitrage. `test_aeolus.cell` 99/99 ; coordination 227, escalade 270, conformite 338, passerelle 370,
bout_en_bout 182, manoeuvres 231 : tous verts ; `check prod.cell` sans erreur ; cycles.py : profondeur 23.

15. **Après un accord tacite, l'escalade d'un muet se clôturait « repondu », puis renaissait tous les
    battements (escalade ↔ coordination ↔ passerelle)** — symptôme : ESC-1 clos « repondu » au battement de
    l'accord, puis un nouveau dossier toutes les 30 min (ESC-2…ESC-150), chacun clos au battement suivant,
    avec sa relance ; aucune montée cohérente vers le chef de mission ni le point focal, statistiques
    d'escalades faussées. Cause : `EscDossiers._a_repondu` tenait `ApiCoordination.decidee(r)` pour une
    réponse de l'opérateur ; ce n'en est pas une quand la décision est prise sans lui. Correction
    (`sys/escalade/chair/dossiers.cell`) : « répondu » = op ∉ `ApiCoordination.muets(r)` ; décision arrêtée
    sans lui : l'escalade continue pour INFORMER (montée selon le barème, pas de reprise à l'échéance
    finale), close « informe » par son accusé, caduque au TCA ou à la clôture du rapprochement ;
    commentaires de `EscDossiers.accuser`, `clore`, `balayer` (`sys/escalade/dossiers.cell`), `--amend`.
    Rejeu : e1 15 h, e2 15 h 30, e3 16 h, e4 31/08 1 h, jamais de reprise, décision intacte.
16. **Un même silence compté plusieurs fois (passerelle ↔ conformite)** — symptôme : deux manquements « autre »
    de gravité 3 contre SpaceX à 14 h 30 (alertes du 30/08 non acquittées avant même l'échéance de réponse),
    puis quatre de plus après la clôture du rapprochement, en plus du « reponse_tardive ». Cause :
    `PasMessages._suivre_defaut` signalait à la conformité tout défaut dont l'escalade était refusée, y
    compris NonEligible (op pas encore muet : la coordination constatera elle-même le silence) et Ferme
    (rapprochement clos). Correction : `sys/passerelle/chair/messages.cell` (ces deux refus : défaut seulement
    journalisé) ; commentaire de `PasMessages.traiter_file`, `--amend`. Rejeu : un seul « reponse_tardive »
    (MQ-5) ; reste « refus_escalade » (MQ-6, veille de conformite sur l'escalade sans accusé), voulu.

Constaté, non corrigé : l'accord tacite ne vaut qu'information « notifiée » pour le muet — SpaceX reçoit la
décision sur son webhook mais ne l'acquitte jamais ; ORBITE ne peut garantir qu'il ne manœuvrera pas de son
côté (risque résiduel de 2019). La décision est prise sans quatre yeux d'ORBITE (mode accord, valideur ''),
alors que l'arbitrage d'un silence en exigeait : choix de conception à confirmer.
