# observation — notes d'architecture

## Choix de conception
- 7 fichiers, 8 cells, 8 machines : ObsFournisseurs (agrement), ObsCapteurs (cycle), ObsTaches (tache),
  ObsMesures (mesure), ObsOrbites (determination), ObsPistes (piste), ObsDetections (detection),
  ObsCouverture (couverture, réactive, une instance par régime).
- Chaîne de preuve : mesure (capteur lié) → détermination (cite ses mesures + l'éphéméride comparée et l'écart)
  → détection / catalogage. `ObsOrbites.fondement` restitue cette chaîne, filtrée par ApiDonnees.peut_recevoir et
  tracée (ApiDonnees.tracer) ; une mesure citée n'est jamais purgée (propriété 6).
- Liens écrits une fois : capteur→fournisseur, tâche→capteur, mesure→capteur, piste→orbite, détection→orbite.
  Une tâche ne change jamais de capteur : panne ⇒ « interrompue » + tâche de relève (origine_de).
- Les délais sont des états tenus par des rondes `par = "systeme"` : ObsCapteurs.veiller (silence 6 h,
  étalonnage 30 j), ObsTaches.expirer, ObsOrbites.perimer (72 h), ObsPistes.veiller (7 j),
  ObsDetections.veiller (une suspicion non jugée en 12 h est confirmée d'office ; un plan invoqué mais
  non produit sous 24 h ⇒ signalée), ObsCouverture.evaluer (incident 3 si un régime devient aveugle).
- Règles internes (via) : ObsCapteurs en_etalonnage→en_service when ObsFournisseurs.agree ;
  ObsTaches affectee→en_cours when ObsCapteurs.en_service ; ObsMesures recue→validee when ObsCapteurs.en_service ;
  ObsPistes confirmee→cataloguee when ObsOrbites.validee ; ObsDetections suspectee→confirmee when
  ObsOrbites.validee || remplacee. verify : 0 ✗ ; les 5 ⚠ « open obligation » portent sur l'écriture unique des
  slots via par des trous ; les ⚠ « runtime-checked » de taille sont ceux des trous (require à poser dans les corps).

## Règles entre sous-systèmes proposées
- Cible = ObsCapteurs.en_service ; exige = donnees:adhésion du fournisseur active (ApiDonnees.adherent) ; lien = fournisseur_de.
- Cible = ObsPistes.cataloguee ; exige = ref:objet créé au catalogue (RefObjet catalogué) ; lien = objet_de (piste → objet).
- Cible = <ref>.objet catalogué de nature debris/inconnu sans lancement ; exige = observation:ObsPistes.cataloguee ; lien = objet → piste.
- Cible = <manoeuvres>.plan vérifié après exécution ; exige = observation:ObsOrbites.validee d'époque > exécution ; lien = plan → OD.
- Cible = <conformite>.manquement "manoeuvre_non_annoncee" constaté ; exige = observation:ObsDetections.signalee ; lien = référence → détection.
- Cible = <coordination>.désignation arrêtée ; exige = observation:aucune ObsDetections.confirmee non jugée sur l'objet désigné ; lien = r → objet.

## Phase 1b (contrat v2)
- Façade : 11/11 signaux branchés (facade.txt) ; balayer → ObsBalayage.balayer (seule entrée du temps, aucun now/every).
- ecart_a / perigee / apogee : ObsOrbites garde vecteur d'état, périgée, apogée par détermination.
- deposer_mesure → ObsMesures.recevoir (grandeur déduite du capteur, empreinte calculée, refus Suspendu).
- Trous fermés : ApiRef.genre à l'agrément ; détection confirmée → ApiManoeuvres.plan_couvrant (expliquée)
  sinon ApiManoeuvres.constater_non_annoncee (plus d'appel direct à conformite/éphémérides) ; piste cataloguée
  au nom d'ORBITE puis ApiRef.attribuer ; purge gardée par ApiResponsabilite.sous_gel("OBJ-…" et "J:<jour>") ;
  validation d'orbite sur ApiEphemerides.depot_valide ; ApiEvenements.evenement_de pour les fragments.
- Objets jamais observés : ObsOrbites.perimer parcourt ApiRef.catalogue par pages. Demandes : aucune.

## Phase 2 (règles entre sous-systèmes, ephemerides + observation)
- Écrites : 2, forme arête. (1) EphDepot.admis ⇐ OpMembres.actif via operateur_de (EphDepot.admettre ; refus Suspendu ; calls ApiOperateurs.est_actif).
  (2) ObsFournisseurs.agree ⇐ RefOrg.active, même id Org (ObsFournisseurs.agreer, demande -> agree ; calls ApiRef.organisations, ApiRef.genre) — remplace la règle « adhésion donnees », impossible.
- Structure : EphDepot gagne l'état `admis` (recu -> admis, admis -> validee, admis -> rejetee) et le signal `admettre`
  (appelé par deposer entre creer et examiner, dépôts "proprietaire" seulement : un dépôt "observation" reste recu -> validee,
  ORBITE garde la trajectoire d'un objet d'opérateur suspendu). valider garde `when EphReglage.en_vigueur via reglage_de` sur les deux arêtes. facade.txt mis à jour.
- verify (1 passage) : règle (2) ✓ ; règle (1) « open obligation … operateur_de written by the hole creer » (acceptable) ;
  aucun ✗ ni « not explored » sur mes cells (seul ✗ global : EscVeille.couverte, escalade). check 0 erreur ; cycles.py : aucune composante, profondeur max 23.
- Écartées : EphDepot.validee ⇐ objet ni rentré/fragmenté/retiré (ref) : 4 états admis (catalogue|en_service|hors_service|cimetiere), explosion ; contrôle à l'exécution (objet_clos).
  ObsCapteurs/ObsFournisseurs ⇐ DonAdhesions.active : id AD-n inconnu (façade = adherent(org) -> Bool, adhésion renouvelée = nouvel id) ; contrôle ApiDonnees.adherent gardé.
  ObsPistes.cataloguee ⇐ RefObjet catalogué : lien objet_de inconnu à la création (l'objet naît dans cataloguer ; 2 écrivains) et vrai par construction.
  EphContestation.acceptee ⇐ suivi d'observation terminé : aucun getter de façade sur l'état d'une tâche (ObsTaches), lien écrit par instruire (facultatif).
  EphDepot.archivee ⇐ RespPurges.autorisee : id de purge inconnu à la création (garde actuelle : sous_gel + EphRetention.libre).
  « gel ⇒ EphRetention.retenu » : implication, pas une précondition ; liberer est une arête de retour.

## Phase 3 (corps)
- Signaux remplis : 95/95 (8 cells internes + ObsBalayage + ApiObservation 11/11), ~1 320 lignes de corps (sys/observation/chair/*.cell, 8 fichiers). Aucun trou laissé.
- Tests : test_observation.cell, 23 tests / 244 assertions, tous verts (`$S test`, 0,9 s). Cycles de vie des 8 machines, refus (habilitation, portée, quatre yeux, état, bornes), règles via (agree, en_service, validee, validee||remplacee, RefOrg.active), scénario complet par la façade (suivi → mesures → OD → manœuvre non annoncée confirmée d'office par balayer → signalement). Doublures : doublures/observation.cell (ApiAcces, ApiRef, ApiDonnees, ApiOperations, ApiMeteo, ApiEphemerides, ApiManoeuvres, ApiPasserelle, ApiConjonctions, ApiEvenements, ApiLancements, ApiResponsabilite).
- Cadres corrigés (internes, amendés, check app.cell OK) : `reads fournisseur_de` sur ObsCapteurs.noter_mesure ; `reads demande_le` sur ObsFournisseurs.agreer et .retirer ; `reads objet_de` sur ObsTaches.compter ; `reads fin_de` sur ObsTaches.interrompre_capteur (relève des seules tâches non échues) ; `reads tache` sur ObsTaches.relever ; `reads sujet_de` sur ObsOrbites.rejeter ; `calls ObsOrbites.etat` sur ObsPistes.cataloguer (garde de l'arête `when ObsOrbites.validee via orbite_de`, sans getter appelable) ; `reads ouverte_le` sur ObsPistes.abandonner et .objet ; `reads objet_de` sur ObsDetections.infirmer. Tous pour pouvoir lever Inconnu (sinon un id inconnu se confond avec l'état initial).
- Demandes à l'intégrateur : aucune.
- Défauts de conception trouvés : (1) ObsMesures `invariant instant_de ≤ recue_le` empêche de tracer une mesure datée du futur : l'instant futur n'est pas enregistré et valider la rejette (« instant_futur ») sur instant absent. (2) ObsFournisseurs n'a pas de slot pour l'auteur de la suspension (quatre yeux de retablir) : motif_suspension stocke « auteur|motif ». (3) ObsPistes n'a pas de date de confirmation : veiller journalise le nombre de pistes confirmées en attente, sans le seuil de 24 h. (4) ObsPistes.rattacher/cataloguer ne lisent pas ouverte_le : piste inconnue ⇒ EtatInvalide (pas Inconnu). (5) ObsCouverture.evaluer prend la capacité 50 en dur (ObsTaches.TACHES_PAR_CAPTEUR illisible : piège 3). (6) ObsOrbites.ecart_a : propagation rectiligne (vecteur à l'époque), sans sigma météo (ApiMeteo hors cadre). (7) La consigne de cataloguer (État = ApiRef.etat_de(ApiRef.orbite())) contredit ApiRef.cataloguer (État "" pour un objet non attribué) : la fiche d'observation est suivie.
- Choix de corps : piste compatible = piste ouverte la plus récente dont la dernière mesure est à ≤ 24 h ; passes = capteurs distincts ; qualité d'une OD = sigma inflaté ≤ 100 m haute, ≤ 1 000 m moyenne, sinon faible ; demander essaie les capteurs les moins chargés puis, en priorité 3, préempte la tâche de plus basse priorité (puis plus petit id) ; demander_suivi (façade) déduit la catégorie du premier mot du motif (« rapprochement CJ-5 »), sinon "routine" ; un objet sans éphéméride valide son OD avec eph = "" ; fondement d'une piste non cataloguée : Confidentiel ; balayer additionne aussi le nombre de régimes non nominaux (evaluer ne rend que lui).
- Limites de Soma rencontrées : les appels nus homonymes (valider, relever, signaler, expliquer) donnent des avertissements d'ambiguïté avec d'autres cells ; les helpers de test doivent porter des noms uniques dans tout le programme (`orbite` ambigu avec ApiRef/RefOrg) ; types nominaux stricts (Objet/String, Duree+Horodatage) à convertir explicitement.
