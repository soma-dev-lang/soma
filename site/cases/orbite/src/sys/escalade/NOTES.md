# escalade — notes d'architecture

## Choix de conception
- 10 fichiers, 10 cells, 8 machines : EscBareme (bareme), EscDossiers (escalade), EscVeille (veille), EscRelances (relance),
  EscDiplomatie (demarche), EscStatistiques (vigilance, réactive), EscPriseEnCharge (prise), EscConcertation (concertation) ;
  sans machine : EscContacts (résolution des contacts sur ApiOperateurs.contacts), EscBalayage (seule entrée du temps).
- Propriété 5 : (1) échéance finale par dossier = TCA − max(6 h, préavis de propulsion), qui ne recule jamais ; passée,
  le balayage met le dossier en REPRISE même s'il a été accusé ; (2) EscVeille arme d'office les ApiConjonctions.urgents de
  niveau 3 et ouvre les dossiers contre les ApiCoordination.muets ; (3) EscPriseEnCharge : la reprise doit être prise par un
  analyste en 15 min, sinon incident de gravité 3 et alerte de direction.
- Échelons = états e1..e4 ; entrée directe à un échelon élevé si le temps manque ; échelon sans contact = délai nul + constat.
- Propriété 6 : forme complète d'un message seulement si ApiDonnees.peut_recevoir pour les deux objets, sinon réduite ; envoi
  complet tracé ; reprise ⇒ ApiResponsabilite.geler ; purge des statistiques suspendue si sous_gel("CJ-…").
- Règles internes (cadres) : dossier `reprise -> close` when EscVeille.couverte via rapprochement_de ; démarche
  `projet -> preparee` when EscDossiers.reprise via dossier_de ; prise `inexistante -> en_attente` when EscDossiers.reprise ;
  prise `* -> tranchee` when EscVeille.couverte via rapprochement_de ; concertation `aucune -> convoquee` when EscDossiers.e4.

## Règles entre sous-systèmes proposées
- Cible = EscVeille.couverte ; exige = coordination:decidee ; lien = r (même id)
- Cible = EscDossiers.reprise ; exige = conjonctions:rapprochement non clos ; lien = EscDossiers.rapprochement_de
- Cible = Coordination.arbitrage (silence) ; exige = escalade:ApiEscalade.reprise(r) ; lien = r
- Cible = Conformite.<manquement silence_escalade> ; exige = escalade:EscStatistiques.vigilance ; lien = op
- Cible = EscDiplomatie.transmise ; exige = ref:État de tutelle participant ; lien = EscDiplomatie.etat_vise

## Avertissements expliqués
- ⚠ invariants entre slots (?? 0) : écrire d'abord le slot de droite (echeance_finale, derniere_ronde, ouvertures, limite, derniere_a).
- ⚠ monotonie `x <= (x.get(key) ?? value)` (echeance_finale, dernier_jour) sans type connu : runtime-checked, voulu.

## Phase 1b
- Façade : 6/6 signaux branchés (facade.txt) ; reprise → EscDossiers.reprise_sur (slot dossiers_de r → dossiers),
  balayer → EscBalayage.balayer (ronde, dossiers, concertations, prises en charge, quotidien : démarches, purge).
- Horloge retirée : 8 `every` et tous les tic*/now_ms supprimés ; EscVeille.en_souffrance supprimé (sans objet : la ronde
  est appelée par l'horloge centrale).
- Trous fermés : « a répondu » = op ∉ ApiCoordination.muets(r) ou ApiCoordination.decidee(r) ; couverture de la veille sur
  decidee (inclut « ne pas manœuvrer ») ; veille armée d'office sur ApiConjonctions.urgents(h, 7 j, 3) ; dossiers d'office
  contre les seuls muets ; propriétaires sans coordination filtrés par ApiRef.peut_manoeuvrer ; contacts par
  ApiOperateurs.contacts (annuaire interne EscContacts supprimé : doublon d'operateurs) ; relance `emise -> echouee`
  sur ApiPasserelle.echoue ; notifier(h, dest, r, …) partout.
- Chaîne de la règle e4 raccourcie : EscConcertation.convoquer(h) est un balayage qui n'émet rien (les messages de l'échelon 4
  valent convocation) et n'appelle que EscDossiers.en_e4/rapprochement, ApiConjonctions.objets, ApiRef.proprietaire/etat_de.
- Lignes : 852 internes (sous 900 après suppression du doublon d'annuaire).
- Demandes : app.cell doit ajouter `use sys::escalade::balayage` (fichier nouveau). ApiOperateurs.contacts ne donne pas la
  direction de l'opérateur (ancien rang « direction » de l'échelon 2) : ajouter un échelon 2b ou inclure
  "administrateur_operateur" à l'échelon 2.

## Phase 2 — règles entre sous-systèmes (escalade + conformite)
- Écrites : 8 règles (11 arêtes + 1 table toml), toutes dans le cadre du signal propriétaire :
  1. EscDossiers.reprise ⇐ ConjRapprochement.alerte || en_coordination via rapprochement_de (arête, ×5 : e1..e4, accusee ; reprendre)
  2. EscDiplomatie.transmise ⇐ RefEtat.participant via etat_vise (arête validee -> transmise ; transmettre)
  3. ConfDemande.executee ⇐ OpMembres.suspendu || OpMembres.exclu via op_de (arête acceptee -> executee ; executer)
  4. ConfDemande.transmise ⇐ while RefEtat.participant via ConfDemande.etat_de (TOML, sys/conformite/regles.toml : arête déjà prise par ConfMesure.decidee)
  5. ConfRapport.transmis ⇐ RefEtat.participant via etat_de (arête valide -> transmis ; transmettre)
  6. ConfContradictoire.forclos ⇐ PasMessages.acquitte via message_de (arêtes ouvert|proroge -> forclos ; forclore)
  7. ConfContradictoire.injoignable ⇐ PasMessages.escalade || PasMessages.signale via message_de (arêtes ouvert|proroge -> injoignable)
  8. ConfManquement.forclos ⇐ ConfContradictoire.forclos || injoignable (même id ; remplace la règle interne à un état)
- Structure : état terminal ConfContradictoire.injoignable (forclusion d'un opérateur que la passerelle n'a pu joindre ; évite 3 alternatives) ;
  arête EscDiplomatie validee -> abandonnee (classer, État devenu non participant) ; caduquer d'un dossier étendu au rapprochement retombé
  hors alerte sans coordination (sinon bloqué hors de la reprise). Aucun slot ajouté (liens existants, écrits une fois à la création).
- Getters ajoutés aux calls : ApiConjonctions.etat (EscDossiers.reprendre), ApiRef.etat_participant (EscDiplomatie/ConfDemande/ConfRapport.transmettre),
  ApiPasserelle.acquitte/echoue (ConfContradictoire.forclore). cycles.py : aucune composante, profondeur max 23.
- Verify (2 passes, copie scratch avec regles.toml fusionné dans soma.toml) : règles 1-8 sans ✗ ; seules « open obligation … written by the hole »
  (preparer/ouvrir : lien écrit une fois, jugé au corps). VERIFY OK à la 1re passe hors règle écartée ci-dessous.
- Écartées : EscVeille.couverte ⇐ CoordDossier.arretee (même id) — essayée sur ronde puis sur un signal dédié : « not proven, calls nest deeper
  than 12 (through EscVeille.armer) » puis ✗ (cône de la machine veille trop profond : ronde → EscDossiers.ouvrir → …) ; retirée, veille.cell
  revenu à l'état de phase 1. À reprendre en sortant armer/ouverture de dossiers de la ronde, ou en forme toml.
  Manquement « silence_escalade » ⇐ EscStatistiques.vigilance, et « manoeuvre_non_annoncee » ⇐ ObsDetections.signalee : une règle d'état ne
  peut viser une nature de manquement (ConfManquement.constate est l'état initial, commun à toutes les natures) — à tenir au corps.

## Phase 3 — corps
- Signaux remplis : 68/68 (62 internes sur 10 cells + 6 de façade ApiEscalade) ; 1 271 lignes de corps (sys/escalade/chair/*.cell, 11 fichiers).
  banc_escalade.cell, test_escalade.cell, doublures/escalade.cell (ApiAcces, ApiRef, ApiConjonctions, ApiCoordination, ApiOperateurs,
  ApiPriorites, ApiConformite, ApiDonnees, ApiPasserelle, ApiResponsabilite, ApiOperations ; sans état, marqueurs dans les ids).
- Tests : 22 cells de test, 270 assertions, toutes vertes (`$S test test_escalade.cell`). Scénario complet : e1 → e2 → e3 → e4 par le
  balayage, concertation convoquée puis tenue, reprise à l'échéance finale, prise en charge, démarche validée/transmise, décision arrêtée
  (mock ApiCoordination.decidee) → veille couverte, dossier clos « decision_arretee », prise tranchée, réponse de l'État.
- Cadres corrigés (amendés, `$S skeleton app.cell --amend`, check app.cell OK, cycles.py sans composante) :
  1. EscRelances : slots `rapprochement_de`, `texte_de` (écrits par emettre, lus par relancer_dues) — sans eux, la réémission ne pouvait
     pas rappeler ApiPasserelle.notifier (r et texte inconnus).
  2. EscPriseEnCharge.suivre : `reads rapprochement_de` (sinon aucune prise à énumérer pour trancher).
  3. EscDiplomatie.transmettre : `calls ApiConjonctions.objets` (ApiDonnees.peut_recevoir exige les objets) ; EscDiplomatie.preparer :
     `calls EscDossiers.etat` (la règle `projet -> preparee when EscDossiers.reprise` est maintenant tenue au corps, refus Inconnu).
  4. EscBareme.adopter / abandonner : `reads redacteur` (refus Inconnu).
- Défauts de conception trouvés :
  - EscPriseEnCharge.limite = min(ouverte + 15 min, max(échéance finale, ouverte)) vaut toujours `ouverte` (la reprise survient à
    l'échéance finale ou après) : alerte de direction immédiate, le délai de 15 min ne servait jamais. Corps : limite = ouverte + 15 min,
    bornée par `fin` seulement si fin > h. Le commentaire du slot est à corriger.
  - Le criblage de la règle `inexistante -> en_attente when EscDossiers.reprise` (ouvrir_prise) n'a pas de getter : tenue par l'appelant
    (rôle "systeme" seul, appelé par reprendre après la transition).
  - compter_accuse / compter_reprise ne lisent pas `ouvertures` : Incoherent est levé en rattrapant l'invariant entre slots (try + fail).
  - EscDiplomatie.classer ne peut pas vérifier que l'État n'est plus participant (pas d'ApiRef.etat_participant) : jugement de la direction.
  - EscConcertation.convoquer ne connaît pas l'opérateur du dossier : `etats` = États des propriétaires dans l'ordre des objets.
  - Une réémission complète n'est pas retracée (relancer_dues n'appelle pas ApiDonnees.tracer) : seule la première émission l'est.
- Demandes à l'intégrateur :
  1. ApiEscalade.escalader sur un dossier accusee / e4 / reprise lève EtatInvalide (via EscDossiers.monter), absent de ses Refus :
     ajouter « EtatInvalide (dossier accusé, en e4 ou repris) » au commentaire, ou `calls EscDossiers.echelon` pour renvoyer l'échelon courant.
  2. ApiResponsabilite.geler : accepter par "systeme" le gel d'une reprise d'escalade (évènement = dossier "ESC-…") ; sinon le gel de la
     propriété 6 est refusé (le corps le rattrape et le journalise, la reprise n'est pas bloquée).
  3. ApiConformite.constater doit accepter les natures "escalade_sans_contact", "reprise_sans_decision", "silence_escalade",
     "demarche_sans_suite" (référence = dossier, "VIG-<op>-<h>" ou démarche).
  4. (rappel 1b) ApiOperateurs.contacts n'a pas la direction de l'opérateur à l'échelon 2.
- Trous laissés : aucun.
- Soma : `mock X []` est une file VIDE (une liste = une file) — pour une liste vide, `mock X [[]]` ; un `mock` en cours de `rules` prend effet
  à partir de cette ligne (utile pour faire statuer la coordination en cours de scénario). `check` avertit « frame says reads …, body never
  reads it » sur relancer_dues (dossier_de, emise_a, forme_de) et accuser (rapprochement_de) : cadres à resserrer, sans gravité.
