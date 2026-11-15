# ORBITE COMMUNE — phase 2 : règles entre sous-systèmes

Le squelette d'ORBITE COMMUNE (20 sous-systèmes, ~20 800 lignes, contrat v2) est intégré dans `app.cell`, se contrôle
et se prouve. Chaque architecte avait proposé, dans `sys/<x>/NOTES.md`, des règles entre
sous-systèmes (forme `Cible = Cell.etat ; exige = <sous-système>:<fait> ; lien = slot`), et parfois
des règles « dans l'autre sens » visant le sous-système d'un autre. environ 100 propositions au total (aussi résumées dans RETOURS_PHASE1.md).
Ta tâche : écrire et faire prouver celles dont la CIBLE est un état d'une cell de TES sous-systèmes.

Binaire : `S=~/fun/soma-banc/binaires/soma-v3.2.4-cone2`
(jamais le `soma` du PATH). Lis `$S docs skeleton` (section « Rules between machines », « A rule on
an edge ») avant d'écrire.

## Méthode

1. Lis les 20 `sys/*/NOTES.md` et relève toute règle dont la cible est dans tes sous-systèmes,
   y compris celles proposées par les autres. Écarte les doublons.
2. Pour chaque règle, identifie la machine et l'état exigés chez l'autre sous-système :
   `$S skeleton app.cell AutreCell` (lecture seule) — ce sont leurs cells INTERNES ; une condition
   `when` les nomme sans les appeler, c'est permis.
3. Forme préférée : sur l'arête, dans le cadre de TON signal :
   `moves a -> b when AutreCell.etat [|| AutreCell.etat2] [via slot]`.
   - Même id des deux côtés (numéro d'objet, id de rapprochement, Org) : pas de `via`.
   - Id différent : un slot de lien dans ta cell cible, écrit UNE fois, par UN seul signal, à la
     création (`writes slot` dans son cadre ; le corps fera `require slot.get(id) == () else Existe`),
     jamais supprimé ; le signal qui porte le `when … via slot` déclare `reads slot`.
   - Une arête ne porte qu'UN `when` (pas de `&&`, pas de `A via x || B via y`). Si l'arête a déjà
     un `when`, ajoute un état intermédiaire (deux arêtes, une condition chacune) ou passe à la
     forme toml.
   - Évite une condition sur une arête qui revient en arrière dans la machine et les alternatives
     à 3 états ou plus : elles font exploser `verify` (> 300 s). Préfère un état de plus.
4. Forme toml (seulement si nécessaire : règle sur le passé `requires`, ou arête déjà prise) :
   écris dans `sys/<toi>/regles.toml` une table `[verify.before."Cell.etat"]` avec `requires = […]`
   ou `while = […]` et `via = "Cell.slot"` si besoin. Une table par état cible. L'intégrateur
   la fusionnera dans `soma.toml` ; tu ne touches pas `soma.toml`.
5. Pour que le corps puisse faire respecter la règle à l'exécution, ajoute aux `calls` du signal
   le getter de façade qui lit le fait exigé (ex. `ApiOperateurs.est_actif`, `ApiCoordination.designe`). Mets à jour son
   commentaire (« Refus : EtatInvalide (…) »).
6. Une règle impossible (lien inconnu à la création, sous-système qui n'a pas la machine voulue,
   besoin de deux conditions sur la même arête sans restructuration raisonnable) : ne la force
   pas, consigne-la.

## Ce que tu possèdes

Les fichiers internes de tes deux sous-systèmes (`sys/<x>/*.cell` sauf `api.cell` et
`entree.cell`) et `sys/<x>/regles.toml`. Rien d'autre : ni `app.cell`, ni `soma.toml`, ni
`commun/`, ni les `api.cell`, ni les dossiers des autres. Le verrou du squelette est retiré pour
cette phase.

## Contrôle

    $S check app.cell                                            # 0,6 s
    perl -e 'alarm 1200; exec @ARGV' $S verify app.cell --summary # ~6 min, machine partagée

`check` : 0 erreur (une erreur dans le fichier d'un AUTRE agent en cours d'écriture est
transitoire : réessaie une minute plus tard). `verify` : lance-le AU PLUS DEUX FOIS (10 agents
partagent 10 cœurs). Chaque règle que tu ajoutes doit sortir ✓, ou « open obligation … written by
the hole » (lien écrit par un signal sans corps : jugé au corps, acceptable). Aucun ✗, aucun
« not explored ».

## Compte rendu

Règles écrites (nombre, liste `cible ⇐ condition [via]`, forme arête ou toml), règles écartées
avec la raison, verdicts `verify` de tes règles, modifications de structure (états, slots ajoutés).
Ajoute la même synthèse à la fin de tes `NOTES.md` sous « Phase 2 ».

## Spécifique à ORBITE COMMUNE

- Le squelette a été gelé puis dégelé pour cette phase (`skeleton.lock` retiré) ; l'intégrateur le regèlera.
- **Priorité absolue** : les règles qui portent les 6 propriétés de `CONSIGNES.md`. En particulier :
  plan d'évitement approuvé ⇐ coordination arrêtée sur cet objet (lien rapprochement du plan) ;
  désignation arrêtée ⇐ objet en service et capable ; manœuvre approuvée ⇐ opérateur actif, éphéméride à jour ;
  rapprochement résolu par manœuvre ⇐ plan vérifié ; coordination ouverte ⇐ rapprochement en alerte ;
  collision ⇐ évènement déclaré ; purge ⇐ hors gel.
- **Aucun cycle d'appels** : n'ajoute aux `calls` qu'un getter de façade qui ne te rappelle pas
  (contrôle : `cd ~/fun/orbite && python3 outils/cycles.py` — il ne doit lister aucune
  composante, seulement les profondeurs ; la profondeur maximale doit rester ≤ 24).
- Les comptes rendus de phase 1 / 1b de chaque sous-système sont dans `RETOURS_PHASE1.md`.
- Ne touche pas `CONSIGNES*.md`, `INTEGRATION.md`, `RETOURS_PHASE1.md`.
