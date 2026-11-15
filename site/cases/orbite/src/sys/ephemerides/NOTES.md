# ephemerides — notes d'architecture
## Choix de conception
- 10 cells, 8 fichiers, 7 machines : EphReglage (versions de seuils, quatre yeux, gel de
  production), EphDepot (cycle d'un dépôt), EphSuivi (situation par objet), EphQuota (réactive),
  EphContestation, EphRetention (réactive), EphTempete (réactive) ; EphControle, EphHistorique,
  EphBilan sans machine.
- Au plus un dépôt "validee" par objet (EphSuivi.courant) ; un dépôt d'époque antérieure est
  validé puis aussitôt "remplacee". Fraîcheur CALCULÉE à chaque lecture (âge max par régime,
  ÷ diviseur si ApiMeteo.tempete) ; l'état "perime" posé par le balayage sert aux relances et
  aux manquements, jamais de source pour `fraiche`.
- Règles internes prouvées : recu -> validee when EphReglage.en_vigueur via reglage_de (chaque
  dépôt garde la version de seuils qui l'a jugé) ; rejetee -> validee|remplacee when
  EphContestation.acceptee (id partagé) ; * -> archivee when EphRetention.libre (id partagé).
- Propriété 6 : EphHistorique.fondement(objet, h) rend le dépôt courant à l'instant d'une
  décision ; un dépôt cité est retenu (EphRetention) et ne se libère pas sous gel
  (ApiResponsabilite.sous_gel) ; consulter passe par ApiDonnees.peut_recevoir + tracer.
- Trous fermés a posteriori (EphControle) : écart à l'orbite observée, anomalie, manœuvre non
  annoncée, plan inclus annulé ⇒ invalidation ; dépôt sans les manœuvres d'un plan actif ⇒
  qualité "faible". Le quota n'arrête jamais un dépôt d'objet en rapprochement de niveau 3 ni
  un dépôt "systeme". Silence d'ORBITE sur une contestation ⇒ caduque, jamais réhabilitation.
## Règles entre sous-systèmes proposées
- Cible = EphDepot.validee ; exige = ref:objet ni rentre, ni fragmente, ni retire ; lien = EphDepot.objet_de
- Cible = EphDepot.validee ; exige = operateurs:opérateur actif (sauf source observation) ; lien = EphDepot.operateur_de
- Proposées aux autres : manoeuvres approuve exige ephemerides:EphSuivi.a_jour (lien = objet du
  plan) ; coordination arretee/arbitrage exige EphSuivi.a_jour ou clos pour l'objet désigné ;
  responsabilite gel ⇒ EphRetention.retenu des fondements (lien = dépôts du dossier).
## Phase 1b (contrat v2)
- Façade : 11/11 signaux branchés (facade.txt) ; nouveaux internes EphDepot.empreinte_du_depot,
  inclut_manoeuvres, est_valide ; EphSuivi.ronde (cible de balayer) ; fondement → EphHistorique.
- Horloge : EphSuivi.tic, `every` et now_ms supprimés ; ronde = tempête, fraîcheur, contrôles,
  contestations, purge, avec le `h` d'Horloge.battre.
- Trous fermés : écart à l'orbite observée jugé À LA RÉCEPTION (ApiObservation.ecart_a ;
  Inconnu ⇒ verdict "sans_temoin", qualité "faible" ; position mal formée ⇒ "format") ;
  plan inclus d'un autre objet (ApiManoeuvres.objet_de) ; listes d'objets fournies par
  l'appelant remplacées par ApiConjonctions.urgents + objets ; manœuvre non annoncée constatée
  via ApiManoeuvres.plan_couvrant / constater_non_annoncee ; purge refusée sous gel de l'objet.
## Phase 1b — demandes
- Aucune bloquante. Confort : `ApiConjonctions.objets_urgents(h, horizon, niveau_min) -> List`.
## Avertissements
- check : len(slot) et invariants entre slots voulus ; verify (phase 1) OK, 1 open obligation (reglage_de écrit par le hole creer).

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
- Signaux remplis : 84/84 (73 internes dont le nouveau EphDepot.motif_rejet_de, 11 de façade) ; ~1 370 lignes de corps (sys/ephemerides/chair/*.cell, 9 fichiers), doublures 226 lignes, tests 426 lignes.
- Tests : 12 `cell test`, 276 assertions, toutes vertes (`$S test test_ephemerides.cell`). Couvrent les 7 machines, les refus (habilitation, portée opérateur, quatre yeux, gel → Delai, états invalides), et le scénario rejet → contestation → instruction → acceptation (4 yeux) → dépôt courant ; plus tempête, purge sous rétention/gel, quota/bridage.
- Cadres corrigés (amendés, `--amend` aussitôt, check app.cell OK, cycles.py : aucune composante) :
  EphDepot.valider + calls EphReglage.courante_version, reads source (exiger la règle `when EphReglage.en_vigueur via reglage_de`) ;
  EphDepot.rehabiliter + calls EphContestation.etat (règle `when EphContestation.acceptee`) ; EphDepot.archiver + calls EphRetention.est_retenu (`when EphRetention.libre`) ;
  EphContestation.accepter + calls EphDepot.etat_de (`when EphDepot.rejetee`) ; EphDepot.deposer + calls EphReglage.age_max, ApiRef.regime, ApiMeteo.tempete (calculer l'age_max passé à valider) ;
  EphDepot.rejeter + reads source (ne pas compter au bilan le rejet d'un dépôt systeme) ; EphReglage.abandonner + reads redige_par (Inconnu) ;
  nouveau signal EphDepot.motif_rejet_de (reads motif_rejet, objet_de) + calls dans EphContestation.contester (seuls "incoherence"/"nee_perimee" contestables) ; EphHistorique.purger + calls EphDepot.fiche (date de dépôt d'un rejeté, qui n'a pas de fin_le).
- Demandes à l'intégrateur : aucune bloquante.
- Défauts de conception relevés : (1) EphTempete.surveiller annonce « Refus Perime … la réaction passe alors renforce » : contradictoire (un refus annule la transition) — le corps passe "renforce" SANS lever Perime (météo muette = tempête par prudence). (2) Le courant d'un objet « perime » n'est lisible ni par invalider_depuis ni par purger : déduit de l'historique (le validé, sinon le périmé d'époque max ; purger protège le dernier validé/périmé). (3) controler_objet : `plan_couvrant` est interrogé à `h` (l'instant de la manœuvre détectée n'est pas exposé par ApiObservation). (4) ronde ne compte que les objets passés "perime" par balayer (tourner rend des objets contrôlés, pas des changements d'état).
- Écarts mineurs : frames plus larges que les corps (confronter : ApiRef.proprietaire et EphDepot.qualite_de inutiles — l'opérateur et la qualité déclarée sont lus dans la fiche ; episode : reads reaction ; invalider_depuis : reads objet_de).
- Trous laissés : aucun.
- Limites de Soma : un refus sur une arête `when` exige un getter de façade dans `calls` (pas de lecture directe de l'état d'une autre machine) ; `transition(id, var, cible)` avec source variable refusé par check → helper `_valider_depuis` à sources littérales ; invariants entre slots imposent l'ordre d'écriture (tolérances de la plus large à la plus serrée, effacées avant correction d'un brouillon ; pris_le avant epoque_courante ; echeance avant ouverte_le) ; typage nominal strict (Duree(idiv(to_int(a), d)), Rapprochement(r), Gravite(g)…).
