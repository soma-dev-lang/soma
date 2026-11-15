# ORBITE COMMUNE — phase 3 : les corps

Le squelette d'ORBITE COMMUNE (20 sous-systèmes, 220 skeletons, 1 775 trous, 94 règles prouvées) est gelé et prouvé
(`VERIFY OK`). Chaque signal a une fiche (« brief ») qui suffit à écrire son corps. Ta tâche :
écrire les corps de TOUS les signaux de TON sous-système — ses cells internes ET sa façade
`Api<Toi>` — et les tester.

Binaire : `S=~/fun/soma-banc/binaires/soma-v3.2.4-cone2`
(jamais le `soma` du PATH). Lis d'abord `$S docs flesh` (une page), puis `$S docs gotchas`.
Travaille dans `~/fun/orbite`.

## Fichiers

Tu possèdes, et toi seul :
- `sys/<x>/chair/*.cell` — tes corps : un fichier `flesh` par fichier de squelette
  (`sys/<x>/ordres.cell` → `sys/<x>/chair/ordres.cell`), et `sys/<x>/chair/api.cell` pour
  `flesh Api<X>`. Un fichier de corps ne contient que des blocs `flesh <Cell> { on … }`.
- `banc_<x>.cell` (à la racine) — ton entrée de contrôle :
  ```
  use app
  use commun::journal_chair
  use commun::horloge_chair
  use sys::<x>::chair::api
  use sys::<x>::chair::<fichier>      // un par fichier de corps
  ```
- `test_<x>.cell` (racine) — tes tests : les mêmes `use` que le banc, plus `use doublures::<x>`.
- `doublures/<x>.cell` — corps PROVISOIRES des façades des AUTRES sous-systèmes que tes corps
  appellent (`flesh ApiAcces { on a_le_role(personne, role) { … } }`, `flesh ApiRef { … }`…),
  pour que tes tests tournent sans eux. Seulement des façades `Api*`, jamais leurs cells
  internes. Ne mets en doublure que ce que tes tests traversent. Conventions conseillées :
  une personne dont le nom commence par "intrus" n'a aucun rôle ; `quatre_yeux(a, b)` est faux
  si a == b ; une organisation dont l'id commence par "ORG-SUSP" est suspendue ; un objet dont le
  numéro commence par "OBJ-MORT" ne peut pas manœuvrer ; ORBITE est "ORG-ORBITE".
- Ton `sys/<x>/NOTES.md` (section « Phase 3 » à la fin).

Le journal commun a déjà son corps (`commun/journal_chair.cell`). Les doublures ne sont
utilisées que par TES tests : l'intégrateur assemblera les vrais corps dans `prod.cell`.

Tu ne touches PAS : `app.cell`, `prod.cell`, `soma.toml`, `commun/`, les `api.cell` (squelettes
de façade), ni les fichiers des autres sous-systèmes.

## Écrire un corps

1. `$S holes banc_<x>.cell <Cell>` donne l'ordre (appelés d'abord).
   `$S skeleton banc_<x>.cell <Cell>.<signal>` donne la fiche : commentaires, slots, invariants,
   constantes, types, chaque machine touchée, et la signature + le commentaire de tout ce que
   le signal peut appeler. Écris le corps depuis la fiche.
2. Le corps fait ce que dit le commentaire, refuse avec les tags qu'il cite
   (`require … else Habilitation "…"` ; les tags valables sont ceux de `Refus` dans
   `commun/types.cell`), et renvoie ce qu'il annonce.
3. **Habilitations** : un signal qui prend `par` vérifie le rôle cité par son commentaire avec
   `ApiAcces.a_le_role` (et `ApiAcces.quatre_yeux(par, valideur)` quand il prend `valideur`), et
   la portée : un rôle d'ORBITE ne vaut que pour un employé de `ApiRef.orbite()`, un rôle
   d'opérateur ou d'État seulement pour son employeur (`ApiAcces.agit_pour`) — voir `Role`.
   `par = "systeme"` est l'identité interne des traitements automatiques (balayages, appels
   entre sous-systèmes) : `ApiAcces.a_le_role("systeme", "systeme")` est vrai.
4. **Règles sur les arêtes** : une arête `moves a -> b when Autre.etat [via slot]` ne se prend que
   si la condition est vraie. Le corps la fait respecter AVANT `transition(...)` par un `require`
   sur le getter de façade que ses `calls` déclarent (ex. `require ApiOperateurs.est_actif(op, h) else
   Suspendu "…"`). Un slot `via` : écrit une fois, à la création
   (`require lien.get(id) == () else Existe "…"`, puis `lien.set(id, …)`), jamais supprimé.
5. **Machines** : `transition(id, "source", "cible")` en nommant toujours la source ;
   `get_status(id) ?? "<initial>"` pour lire l'état.
6. **Journal** : tout geste qui change un état ou un solde appelle `Journal.noter(h, "<x>", par, "…")`
   s'il est dans ses `calls`.
7. Écrire, puis `$S check banc_<x>.cell` souvent (0,6 s). Un corps ne peut toucher que ce que
   son cadre liste : `check` refuse le reste et nomme la clause manquante.

## Si un cadre est faux

Le squelette est gelé : un corps qui ne peut pas être écrit dans son cadre (il lui manque un
`calls`, un `reads`, une arête) est un défaut de conception à corriger, pas à contourner.
- Cadre d'une cell INTERNE de ton sous-système : corrige-le dans ton fichier de squelette, puis
  AUSSITÔT, dans la même commande : `$S skeleton app.cell --amend`. (Une modification de
  squelette non amendée, ou qui ne se parse pas, casse le contrôle des 19 autres agents :
  fais-la d'un coup, vérifie `$S check app.cell`, garde-la rare.) Consigne-la dans tes NOTES.
- Façade (`api.cell`), types communs, règle `soma.toml`, autre sous-système : n'y touche pas.
  Écris le corps au mieux dans le cadre existant, et consigne la demande dans tes NOTES,
  section « Demandes à l'intégrateur », avec la modification exacte.

## Tests

`test_<x>.cell` : des `cell test <Nom> { rules { … assert … } }` (voir
`~/fun/rempart/tests.cell` pour le style). Couvre :
- chaque machine : son cycle de vie nominal de bout en bout ;
- chaque refus de sécurité : habilitation, quatre yeux, portée, état invalide (`assert_fails`) ;
- au moins un scénario métier complet du sous-système (ex. coordination : rapprochement en alerte → proposition →
  accord ou arbitrage → désignation arrêtée → plan approuvé → clôture).
`$S test test_<x>.cell` : tout doit passer. Chaque test part d'une mémoire vide.

## Ne pas faire

- Pas de `soma verify` (6 min, 20 agents sur 10 cœurs) : l'intégrateur le lance sur `prod.cell`.
- Pas de corps creux (`return 0` / `return "ok"` sans faire le travail) : un trou laissé
  honnêtement vaut mieux. Si tu laisses un signal sans corps, dis pourquoi dans les NOTES.
- Pas de git.

## Compte rendu (et section « Phase 3 » de tes NOTES.md)

Signaux remplis / total, lignes de corps, tests (nombre, tous verts ?), cadres corrigés
(amendés), demandes à l'intégrateur (exactes), défauts de conception trouvés, trous laissés et
pourquoi, limites de Soma rencontrées.

## Spécifique à ORBITE COMMUNE

- **Le temps** : jamais `now()` / `now_ms()` dans un corps (sauf `PasEntree.traiter`, déjà prévu) ;
  `h` vient de l'appelant ; les échéances se jugent dans `balayer(h, par)`.
- **Ordre imposé** : reentrees passe sa campagne en « rentree », evenements confirme l'évènement,
  fin_de_vie note la capture AVANT d'appeler `ApiRef.changer_etat_objet(h, objet, cible, reference, …)`,
  avec la référence de la cause (campagne, évènement, mission).
- **Pièges connus de Soma 3.2.x** (banc ~/fun/soma-banc) — à éviter :
  1. `require !a || b else …` est lu `require !(a || b)` : écris `require (!a) || b` ou un `if`.
  2. Une variable locale qui porte le nom d'un slot ou d'un signal d'une AUTRE cell perd ses
     `.push` sans erreur : nomme tes listes locales distinctement (`lignes_r`, `res_x`…).
  3. `AutreCell.CONSTANTE` passe `check` mais échoue à l'exécution : n'utilise que tes constantes.
  4. `xs = xs ++ [v]` en boucle donne un avertissement quadratique : acceptable sur des listes bornées.
- **Criblage natif (conjonctions seulement)** : le criblage tout-contre-tout d'une campagne est écrit
  en `[native]` (`$S docs native`) : une boucle native sur des tampons `buffer_f` (positions et
  vitesses décodées d'une chaîne), propagation simple et distance minimale par paire, sortie en
  chaîne « a,b,tca,distance;… ». Si ton squelette n'a pas de signal pour ce noyau, ajoute-le dans
  ta cell interne (avec --amend immédiat). Teste-le sur un petit catalogue synthétique dont le
  résultat se calcule à la main.
- **Cycles** : n'ajoute aucun appel qui crée un cycle (`python3 outils/cycles.py`).
- `banc_commun.cell` (journal + horloge) existe ; le journal et l'horloge ont déjà leurs corps.
