# acces — notes d'architecture (phase 1, squelette)
## Choix de conception
- 8 fichiers, 13 cells, 10 machines (Personnes, Rattachements, Attributions, Facteurs, Sessions, Jetons, Urgences, Campagnes,
  Attestations, Departs) ; sans machine : AccesPortees (table des portées), AccesControle, AccesBalayage.
- Point de décision unique : AccesControle ; la façade y délègue, les cells internes n'appellent jamais ApiAcces.
- Employeur immuable (`employeur_de` écrit une fois) : changer d'employeur = départ + nouvel identifiant. Portée vérifiée à
  l'inscription (AccesPortees.compatible) ET à chaque contrôle (rôle d'ORBITE ⇒ employeur == ApiRef.orbite()).
- Trou fermé « exploitant faussaire » : une personne inscrite par ORBITE pour un opérateur ou un État n'a de rôle effectif qu'après
  confirmation par son employeur (AccesRattachements ; à défaut d'administrateur, l'autorité de l'État de tutelle ; première autorité
  d'un État : direction d'ORBITE à quatre yeux sur pièce). Sans réponse en 14 j : expiré, personne suspendue.
- Urgence : liée à un rapprochement de niveau 3 non clos (`rapprochement_de`), ≤ 4 h, une par personne, ne change PAS a_le_role
  (lue par en_urgence), revue a posteriori indépendante ; non conforme ⇒ suspension + manquement (ApiConformite.constater) ;
  revue en retard de 72 h ⇒ incident (ApiOperations.declarer_incident, par "systeme").
- Jetons : un par session, secret rendu une fois, empreinte seule stockée. Temps : aucun lecteur d'horloge dans acces (v2).
- Règles internes (arêtes) : personne → partie when AccesDeparts.effectif ; départ → clos when AccesPersonnes.partie ; attribution
  accordée, session ouverte, urgence ouverte when AccesPersonnes.active via titulaire_de / personne_de_session / beneficiaire ;
  facteur enrôlé, rattachement confirmé when AccesPersonnes.active (même id) ; jeton émis when AccesSessions.ouverte via session_du_jeton ;
  attestation confirmée/retirée when AccesCampagnes.ouverte via campagne_de.
## Règles entre sous-systèmes proposées
- Cible = AccesPersonnes.active ; exige = ref:organisation employeur inscrite (non radiée) ; lien = AccesPersonnes.employeur_de
- Cible = AccesRattachements.confirme ; exige = ref:organisation employeur inscrite ; lien = AccesRattachements.employeur_du
- Cible = AccesUrgences.ouverte ; exige = conjonctions:rapprochement ouvert (niveau 3) ; lien = AccesUrgences.rapprochement_de
- Cible = AccesCampagnes.ouverte ; exige = ref:organisation inscrite ; lien = AccesCampagnes.org_de
## Points ouverts
- "auditeur" sans portée dans types.cell : classé rôle d'ORBITE (lecture, revues d'urgence).
- a_le_role n'a pas d'horloge : attributions échues bornées par le balayage (60 s). a_le_role ne lit pas
  ApiOperateurs.est_active (un opérateur suspendu doit encore répondre) : aux manœuvres de refuser.
## Avertissements expliqués
- ⚠ `invariant x.size <= MAX_…` runtime-checked : chaque corps qui ajoute pose `require len(x) < MAX_… else Plein` avant son unique ajout.
- ⚠ `ensures` sur retours String typés et invariants d'énumération : runtime-checked par construction (trous).
- ⚠ open obligation × 6 « via … written once » : chaque slot lien est écrit par un seul signal créateur ; jugé aux corps.
- check : invariants entre slots → écrire d'abord le slot de droite ; plus aucun `every`.
## Phase 1b (contrat v2)
- Façade : 16/16 signaux branchés (facade.txt). Nouveaux : en_urgence_pour → AccesUrgences.en_urgence_pour ;
  ouvrir_session → AccesSessions.ouvrir (+ AccesJetons.emettre, canal déduit) ; authentifier → AccesJetons.authentifier(jeton, h) ;
  fermer_session → AccesSessions.fermer(h, jeton, par) ; actifs_de → AccesControle.actifs_de ; balayer → AccesBalayage.balayer.
- Trous fermés : urgence liée à r (niveau 3 + état ouvert, plus de lecture du motif) et en_urgence_pour ; genre lu par ApiRef.genre.
- Horloge : AccesHorloge, every et now_ms supprimés ; AccesBalayage.balayer (ronde quotidienne gardée par `dernier_jour`).
- Demande : ApiAcces.ouvrir_urgence — ajouter `Doublon` (urgence déjà ouverte) et `Plein` à son « Refus : ».

## Phase 2 — règles entre sous-systèmes (ref + acces)
- Écrites (8, forme arête sauf mention) : RefObjet.rentre ⇐ ReeCampagne.rentree||close via rentree_de (4 arêtes) ; RefObjet.fragmente ⇐ EvtEvenements.confirme||clos via fragmentation_de (4 arêtes) ; RefObjet.retire ⇐ FdvRetrait.capturee via retrait_de (2 arêtes) ; RefObjet.cimetiere ⇐ FdvFin.passive||en_retard (même id) ; AccesPersonnes.active (inconnue→active) ⇐ RefOrg.inscrite||active via employeur_de ; AccesCampagnes.ouverte ⇐ RefOrg.inscrite||active via org_de ; toml (sys/acces/regles.toml, arête déjà prise) : AccesRattachements.confirme ⇐ while RefOrg.inscrite||active via employeur_du ; AccesUrgences.ouverte ⇐ while ConjRapprochement.alerte||en_coordination via rapprochement_de.
- verify (copie de travail + regles.toml fusionné, 2 passes) : cimetiere ✓ (54 états produits) ; les 7 autres « open obligation … written by the hole » (lien écrit par un trou) ; aucun ✗ ni « not explored » sur ref/acces (seul ✗ global : EscVeille, pas à nous). cycles.py : aucune composante, profondeur max 23.
- Écartées : RefOrg.radiee ⇐ OpMembres retire|exclu|jamais candidat (3 alternatives dont l'état initial pour les orgs non opérateurs ; tenu au corps par ApiOperateurs.statut) ; RefCession.realisee ⇐ aucun plan approuvé non vérifié (négation sur un ensemble de plans, pas un état) ; RefObjet.en_service ⇐ LancPiece.identifiee|immatriculee (arête déjà gardée par RefRegistre, lien origine_de chez lancements, objets hors lancement) ; objet catalogué ⇐ ObsPistes.cataloguee (état initial = création, pas d'arête ; plusieurs créateurs) ; propulsion non redéclarable après passivation (pas un état : RefObjet.modifier appelle ApiFinDeVie.passive, refus NonEligible) ; cimetiere depuis FdvFin.defaillant (3e alternative écartée : coût de verify ; le requires toml « passé par en_retrait|defaillant » sortait ✗ sur le trou).
- Structure : slot `causes` remplacé par rentree_de / fragmentation_de / retrait_de (un écrivain chacun, immuables ; cause() lit les trois) ; calls ajoutés : placer_cimetiere + ApiFinDeVie.passive/en_retard, modifier + ApiFinDeVie.passive, AccesRattachements.ouvrir/confirmer + ApiRef.organisation_existe ; commentaires de refus mis à jour.
- ORDRE (phase 3) : reentrees passe la campagne en « rentree », evenements confirme l'évènement AVANT ApiRef.changer_etat_objet ; la façade doit fournir la référence (id campagne / évènement / mission) aux signaux internes. Getters manquants : ApiReentrees.campagne_de(objet), ApiFinDeVie.mission_de(objet), ApiEvenements.confirme(ev), état defaillant de FdvFin.

## Phase 3 — corps
- **Bilan** : 110/110 signaux remplis (13 cells internes + ApiAcces), 0 trou ; ~1 230 lignes de corps (`sys/acces/chair/*.cell`, 9 fichiers) ;
  `banc_acces.cell` : 0 erreur. `test_acces.cell` : 13 cellules, 252 assertions, toutes vertes. Doublures (`doublures/acces.cell`) :
  ApiRef (orbite, organisation_existe, genre, etat_de, etat_participant, org_de_etat), ApiConjonctions (etat, niveau),
  ApiConformite.constater, ApiOperations.declarer_incident — conventions en tête du fichier.
- **Cadres corrigés (amendés, `skeleton app.cell --amend`, `check app.cell` OK)** : AccesDeparts invariant
  `(effet ?? 0) <= (parti_le ?? 0)` → `(parti_le ?? 9999999999999)` (annoncer / partir_maintenant écrivent effet sans parti_le : le
  départ était impossible) ; AccesDeparts.annoncer, annuler `calls + AccesPortees.genre_de` (« exploitant pour un employé d'ORBITE ou
  d'un État ») ; AccesAttributions.refuser `calls + AccesControle.agit_pour, AccesPersonnes.employeur` (chef_de_mission du même employeur).
- **Demandes à l'intégrateur** : (1) ApiAcces.ouvrir_session — remplacer « Habilitation (facteur non utilisable ou invalide) » par
  « Habilitation (facteur non utilisable) ; renvoie "" si le facteur est faux (l'échec est compté) » : lever annulerait le compteur
  d'échecs de AccesFacteurs.verifier (geste atomique) et laisserait la force brute ; même chose pour AccesSessions.deverrouiller
  (renvoie "verrouillee"). (2) fusionner `sys/acces/regles.toml` (phase 2) dans soma.toml.
- **Défauts de conception trouvés** : premier exploitant impossible (enregistrer n'accepte que l'exploitant pour les rôles d'ORBITE
  hors les quatre) → la direction inscrit aussi tout rôle d'ORBITE (corps de _habilite_pour, AccesPersonnes et AccesAttributions) ;
  relancer_revues n'a aucune mémoire d'écriture : « une fois » tenu par une fenêtre d'un jour après le délai (ronde quotidienne) — un
  slot `relancees` serait plus sûr ; MAX_SESSIONS / MAX_JETONS / MAX_URGENCES comptent tout l'historique (liens jamais effacés) : le
  registre sature, prévoir une purge ; la règle « rattachement confirme when AccesPersonnes.active » est tenue dans ouvrir par
  AccesControle.agit_pour(personne, employeur) (pas de AccesPersonnes.est_active dans ses calls) ; contester n'a pas personnes_de :
  l'autorité de tutelle peut toujours contester (geste protecteur) ; un fournisseur de capteurs n'a que le rôle "capteur" (compatible),
  donc jamais d'administrateur : ses personnes sont confirmées par l'autorité de tutelle ; ApiAcces.suspendre (motif libre) → motif
  typé s'il nomme un AccesMotifSuspension, sinon "autre" avec le texte en détail.
- **Non testé** : départ d'office sur employeur radié (les doublures de ref sont sans état : un employeur ne peut être radié en cours de test).
- **Limites de Soma** : types nominaux (Horodatage, Org, Role, AccesGenre…) : toute arithmétique passe par to_int(), toute valeur
  lue d'une Map se reconvertit (`Role(d)`, `AccesCanal(c)`) ; pas d'index secondaire : roles_de, fermer_toutes, revoquer_session
  parcourent tout le slot (a_le_role est O(attributions)) ; un `try` raté annule ses écritures, d'où la demande (1).
