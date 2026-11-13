# JOURNAL — clinique en Soma, « skeleton first »

Binaire : `r6/bin/soma` (3.0.0). Projet : `build/clinic/`. Expériences : `build/exp/`. Reproducteurs : `build/repro/`.
Convention : [B] bloquant, [C] coûteux, [A] agaçant, [+] ce qui a aidé.

## Phase 0 — découverte de la doc (≈ 10 min)

- [A] `soma docs` sans sujet sort en code 1 (liste des sujets sur stderr). La consigne « `soma docs` (référence) »
  devient `soma docs reference`. Sortie utile, mais un code 0 serait plus honnête pour une page d'aide.
- [A] `soma init --skeleton clinic` crée `clinic/soma.toml` avec des tables vides `[verify.after]`, `[verify.before]`,
  `cells = []`, `deadlock_free = false`, `[compute]`, `[cluster]`, `[agent]`, `[models]`, `[bus]`… 40 lignes de bruit
  dont je n'ai besoin de rien ; il faut deviner lesquelles enlever sans casser.
- [A] `soma docs skeleton`, exemple : le cadre de `Orders.place` dit `calls Stock.take` et la chair écrit
  `take(sku, 1)` (non qualifié). Les deux marchent sans doute, mais la doc flesh dit « as the brief writes it ».
- [C] **Doc contradictoire sur les règles inter-cellules.** `soma docs agent` : « a rule BETWEEN two cells' machines is a
  `[verify.before."Pay.paid"]` property, proven on their product ». `soma docs guarantees` §3 : « Cross-cell rules: NOT
  verified: proofs are per cell […] enforce those rules yourself with a `require` that reads the other cell
  (`require Subjects.status(id) != "withdrawn" else Withdrawn`) ». Et ce `require` recommandé n'est PAS une preuve
  pour le produit (voir expérience e1 ci-dessous).
- [C] **Aucune doc ne dit quelle forme de corps prouve une règle `[verify.before."A.x"] requires ["B.y"]`.**
  J'ai dû le trouver par essais (exp/e1, 6 essais, ~15 min) :
  - `require Consult.etat(id) == "terminee" else …` (etat = `return get_status(id)`) → ✗ non prouvé
    (repro/product_status_call_not_evidence) ;
  - `let ok = Consult.exiger(id)` où `exiger` fait `require get_status(id) == "terminee"` dans SA cellule → ✓ prouvé
    (repro/product_same_id_ok) ;
  - la même chose avec un AUTRE id (`Consult.exiger(o.consult)`, id lu dans un slot) → ✗ (repro/product_other_id).
  Conséquence de conception lourde : le produit suit « un id à travers toutes les machines », donc une règle
  ordonnance→consultation n'est prouvable que si l'ordonnance et la consultation ont LE MÊME identifiant.
  J'ai donc tordu le modèle : un seul id de « visite » sert de clé au rendez-vous, à la consultation, à l'ordonnance
  et à la facture (donc une seule ordonnance et une seule facture par consultation). Le message d'échec ne dit rien
  de tout cela : `✗ before Ordo.delivree requires Consult.terminee: a run of the program's handlers enters
  `delivree` without it (from Consult=ouverte, Ordo=signee)` — il ne dit pas « je ne sais pas relier
  `o.consult` à l'id de l'ordonnance » ni « un test sur la valeur de retour d'un appel n'est pas une preuve ».
- [C] **`reads Cell.machine` est déclarable mais je n'ai trouvé aucune syntaxe de corps pour s'en servir** depuis une
  cellule qui a sa propre machine. Essais : `Consult.get_status(id)` (« cell 'Consult' has no handler
  'get_status' »), `Consult.consult.get_status(id)`, `get_status(id, "Consult.consult")` (« takes at most 1
  argument »)… Le brief imprime pourtant « machine Consult.consult — … get_status(id) is () until… » comme si
  `get_status(id)` lisait cette machine. 7 essais. Contournement : un signal lecteur dans l'autre cellule.
- [A] Je n'ai pas utilisé `soma example` (hors de la liste de doc autorisée) ; `--help` le met en avant.

## Phase 1 — écriture du squelette (≈ 35 min, 14 cellules, 112 signaux, ~700 lignes)

- [A] `initial: demandé` → `error: unexpected character 'é'` (E1010). Vrai, mais ne dit pas « les identifiants sont
  ASCII ». 1 essai. Pour un domaine francophone tous les états perdent leurs accents (`reporte` au lieu de `reporté`).
- [+] Le premier `soma check` du squelette complet (14 fichiers) est passé du premier coup, avec le compte de trous.
  Les cadres se lisent bien à l'écriture ; les `calls Cell.signal` forcent à nommer chaque dépendance.
- [C] **La carte `soma skeleton app.cell` perd tous les commentaires** (445 lignes, 18 Ko pour 112 signaux). Il reste des
  signatures et des clauses ; l'intention (ce que vaut un slot, ce que signifie une arête, l'ordre des erreurs) n'y
  est plus — alors que le brief, lui, les porte. À cette taille la carte ne se lit plus d'un trait : je m'en suis
  servi 2 fois puis j'ai relu mes fichiers `lib/*.cell`, qui sont plus courts par cellule ET commentés.
  Ce que j'ai cherché sans le trouver :
  - une vue d'UNE cellule depuis l'entrée : `soma skeleton app.cell Agenda` → `error: no signal `Agenda`` suivi de la
    liste des 112 signaux sur une ligne (3 Ko). (Contournement : `soma skeleton lib/agenda.cell`.)
  - un index : une ligne par cellule (slots, machine, n signaux, n trous) ;
  - le graphe « qui appelle qui / qui émet quoi / qui écoute » au niveau cellule ; il n'existe que signal par signal
    dans le brief (« called by ») ;
  - les clauses sont réordonnées alphabétiquement (`calls Agenda.annuler_rdv, Agenda.confirmer, …`) : l'ordre que
    j'avais choisi (l'ordre des routes) est perdu.
- [A] `soma skeleton app.cell facture_change` → `error: `facture_change` is a signal of `Notifications` and `Audit`:
  write `Cell.facture_change`` : bien. Mais `soma skeleton app.cell rdv` imprime en tête 3 lignes de commentaire
  de cellule préfixées « // Agenda: Agenda — … » (doublon du nom).
- [C] **Limite d'expressivité du cadre : la règle inter-machines n'a pas de clause.** « `delivrer` prend
  `signee -> delivree` seulement si Consultations est `terminee` » ne peut s'écrire que dans un commentaire ;
  `calls Consultations.exiger_terminee` n'est pas lu comme une garde par verify tant que le corps est un trou.
  Résultat : sur le squelette vide, les 4 règles inter-cellules sortent toutes « ⚠ open obligation » ; on ne peut
  RIEN prouver d'inter-cellules avant d'avoir écrit les corps. Il manque une clause du genre
  `moves signee -> delivree when Consultations.consult in terminee`.

## Phase 2 — soma.toml et `soma verify` sur le squelette vide

- [C] `[verify.after."Agenda.annule"] never = ["Consultations.ouverte"]` →
  `error: soma.toml: [verify.after.<state>] names state 'Agenda.annule', which no verified state machine declares —
  a property about a state that does not exist proves nothing`. Le message est faux (l'état existe) : c'est la forme
  qualifiée qui n'est acceptée que par `[verify.before]`. La doc ne le dit nulle part. 1 essai + réécriture.
  (repro/after_qualified)
- [A] Une propriété non qualifiée (`[verify.after.annule] never = […]`) est appliquée aux 6 machines : 10 lignes
  « ✓ … state 'annule' is unreachable (vacuously true) » qui gonflent le compteur de ✓. Et `cells = […]` est global
  à la table `[verify]`, donc impossible de viser une machine pour une règle et une autre pour une autre.
- [C] Sortie de verify sur squelette vide : 231 lignes. `--summary` : 43 lignes, dont 37 ⚠ qui disent toutes la même
  chose (« because `?` has no known range », « not proven for `?` (unbounded) ») — vrai pour tout trou par
  définition. Les 4 lignes utiles (open obligations du produit) sont noyées en bas. Une seule ligne
  « 37 invariants/contrats : jugés quand les trous auront un corps » suffirait.
- [A] Le contre-exemple d'une open obligation donne l'état du produit (« from Agenda=demande, Consultations=attente,
  Ordonnances=signee, Laboratoire=demandee, Facturation=brouillon, Paiements=initie ») mais ni le chemin ni LE trou
  responsable (`Ordonnances.delivrer`). Les 3 machines sans rapport y figurent.
- [A] Juste sous la section `product:` viennent 3 notes « cross-cell: `Consultations.ouvrir` transitions its own machine
  and acts on `Agenda` — a rule BETWEEN the two cells' machines … is not verified: state it as a `require` reading
  the other cell » — alors que la règle EST déclarée dans soma.toml et listée 8 lignes plus haut. Bruit contradictoire.
- [+] Verify accepte le squelette vide (rc 0, « VERIFY OK (skeleton — the holes above are open obligations) ») et
  prouve déjà les règles mono-machine (`before Facturation.payee requires Facturation.emise`, labo) : ça, c'est
  exploitable avant d'écrire une ligne de corps. 4,8 s pour 16 128 états produits : correct.

## Phase 3 — gel
- [+] `soma skeleton app.cell --freeze` : 1 commande, `skeleton.lock` de 316 lignes lisibles (le squelette normalisé +
  un hash par cellule + soma.toml). Ajouter des `use flesh::…` dans app.cell ne casse pas le verrou : bien vu.

## Phase 4 — la chair (112 corps, ≈ 45 min)

- [+] **Les 112 corps ont passé `soma check` avec 2 séries d'erreurs seulement**, et 182 assertions sont passées avec
  UNE seule assertion rouge (une erreur de mon test). Le cadre fermé m'a forcé à nommer `Audit.consigner`,
  `emits`, etc. à la conception ; à l'écriture il n'y avait plus de décision à prendre.
- [C] **E0111 (« may be absent ») s'applique aux slots typés et pas aux slots `Map<String, Map>`.**
  `praticiens.keys.filter(k => praticiens.get(k).specialite == s)` (slot `Map<String, Map>`) passe ;
  `lots.keys.filter(k => lots.get(k).medicament == m)` (slot `Map<String, Lot>`) → 12 erreurs :
  ``error: `lots.get(k)` may be absent (`()`) when `.medicament` is read — default it (`(lots.get(k) ?? …)`) or test it
  first``. Donc suivre le conseil de la doc (« Prefer a `cell type` over `-> Map` ») est PUNI de 12 erreurs, et le
  code laxiste passe en silence. En plus `k` vient de `lots.keys` : l'absence est impossible, et le défaut proposé
  (`?? …`) n'a pas de valeur sensée pour un record. Contournement trouvé seul (non documenté dans `docs flesh`) :
  `lots.entries.filter(e => e.value.medicament == …)`. 1 aller-retour, ~5 min. (repro/absent_typed_vs_map)
- [B→contourné] **Bug compilateur : `return Rec { … }` est refusé quand la face dit `-> Rec`.**
  ```
  on a(t) { return Montants { total: t, patient: t } }
  error: handler 'a' returns a Map literal but its face declares `-> Montants` — fix the value or the face  (E0040)
  ```
  alors que `let m = Montants { … }  return m` passe. Reproduit aussi dans une `cell` ordinaire
  (repro/record_return/app.cell et plain.cell). Le message est faux (ce n'est pas un Map literal) et ne suggère
  pas le contournement. 2 essais.
- [A] Le brief n'imprime la définition d'un type (`types Lot = …`) que s'il est dans la SIGNATURE. `Pharmacie.sortir`
  écrit `lots: Map<String, Lot>` et son brief ne donne pas les champs de `Lot` ; c'est mon commentaire de slot qui
  m'a sauvé.
- [A] Tests : `use` ne remonte pas (`tests/base.cell` avec `use app` → « no file app.cell beside the program » ;
  `use super::app` → cherche `tests/super/app.cell`). Les fichiers de test doivent être À CÔTÉ de app.cell.
  Et `soma test app.cell` répond « no test cells found » sans dire que les tests des fichiers importés ne tournent
  pas, ni que `soma test .` existe (trouvé par hasard ; `soma test a.cell b.cell` est refusé). La doc agent dit
  « `use` resolves beside THAT file », la référence dit « resolved from the project root » : c'est la racine de
  l'ENTRÉE (le fichier passé en argument) qui gagne. 3 essais.
- [A] `soma verify` avec tous les corps : 17 ⚠ « runtime-checked », presque tous triviaux pour un humain :
  `ensures result >= 0` non prouvé pour `fiches.size`, `journal.len`, `boite.count(…)`, `sum(par_facture.values)`
  (slot avec `invariant >= 0`), `idiv(absents * 100, absents + venus)`. Le prouveur ne sait pas qu'une taille
  est ≥ 0. Conséquence : `--strict` est inutilisable sur ce programme sans réécrire des contrats vrais.
- [+] `soma holes` : compteur fiable (112 → 85 → 43 → 0), et l'ordre « callees first » est sensé. Je ne m'en suis
  servi que comme jauge ; à 112 signaux j'ai rempli cellule par cellule, pas dans l'ordre numéroté (qui saute d'une
  cellule à l'autre : `Patients.inscrire` est n° 92 parce qu'il appelle `Audit.consigner`).
- [+] Briefs : 17 signaux écrits en ne regardant que `soma skeleton app.cell Cell.signal` (sortir, demander,
  emettre, delivrer, payer, request, ouvrir, terminer, compter_statut, cloturer, retirer_perimes, preparer,
  trancher_litige, rembourser, resultat, dossier_visite, totaux). Tous justes du premier coup aux tests.
  Réserve honnête : c'est moi qui avais écrit le squelette 20 minutes plus tôt ; j'ai donc aussi lancé un agent
  NEUF qui n'a que `docs flesh` + 6 briefs (voir « Expérience brief seul » plus bas).
  Ce qui manquait dans les briefs, vu de l'auteur :
  - l'ORDRE imposé entre effets quand il porte une preuve (`terminer` doit transitionner AVANT d'appeler
    `Agenda.cloturer`, qui rappelle `exiger_terminee`) : rien dans le cadre ne le dit, seulement mes commentaires ;
  - ce que fait le signal quand l'instance est inconnue, si le commentaire l'oublie (`trancher_litige`) ;
  - la forme exacte des données d'un `emit` : heureusement le brief recopie le commentaire de l'écouteur ;
  - le brief d'un signal qui lit une autre machine dit « get_status(id) » sans dire que ça ne lit QUE sa propre machine.
  La qualité du brief = la qualité des commentaires du squelette. Sans mes commentaires longs, le brief ne
  contient que des signatures : il « suffit » parce que j'ai écrit la spec en prose dedans.

## Phase 5 — tests
- [+] 182 assertions en 3 fichiers (`test_base`, `test_parcours`, `test_labo_api`), vertes. `assert_fails … matching`
  et `mock now` font le travail ; le message d'échec (source, ligne, état courant, cibles valides, lieu de la levée)
  est excellent : `ERROR: invalid transition: delivree → annulee. Current state: 'delivree'. Valid targets: []
  (raised at flesh/ordonnances.cell:42)`.
- [+] L'atomicité d'un handler (délivrance qui échoue à la 2e ligne → rien n'est sorti du stock) marche sans une
  ligne de code de compensation, à travers 3 cellules.

## Expérience « brief seul » (agent neuf, salle blanche)
Un sous-agent sans aucun accès au projet a reçu `soma docs flesh` + 6 briefs (exp/brief_only/INPUT.txt) et a écrit
les 6 corps (exp/brief_only/OUTPUT.txt). Je les ai substitués dans une copie du projet :
- 5 sur 6 (`Facturation.emettre`, `Api.request`, `Agenda.annuler_rdv`, `Facturation.totaux`, `Ordonnances.delivrer`)
  compilent tels quels et passent TOUTES les assertions existantes. Le brief suffit donc réellement pour ces corps.
- `Pharmacie.sortir` échoue deux fois : (1) 10 erreurs E0111 (`lots.get(k).peremption` sur un slot typé) ; (2) après
  correction mécanique, erreur à l'EXÉCUTION seulement : `slot 'lots': expected Lot, got Map` — l'agent a écrit
  `lots.set(k, map("medicament", …))` parce que ni le brief ni `docs flesh` ne montrent comment construire un `Lot`
  (pas de ligne `types`, pas de littéral de record dans la page). `soma check` ne voit pas ce type faux.
- Ce que l'agent dit avoir dû deviner (toutes des lacunes de `soma docs flesh`) : la syntaxe de `emit` (la page liste
  `emits` mais jamais l'instruction) ; `ends_with`, `sort_by`, `substring` (absents : il a écrit un tri par sélection
  et `contains(kind, "Inconnu")`) ; le littéral de record ; si un appel nu `Cell.f(x)` est une instruction légale ;
  si `[]` est accepté pour `List<Map>` ; ce que fait `split("/a", "/")`.

## Phase 6 — demandes de changement

### (a) état `reporte` + signal `reporter`
- Verrou : `soma check` refuse net et imprime le diff exact (4 arêtes, 1 signal) ; tout est bloqué tant qu'on n'a pas
  amendé — y compris `soma skeleton app.cell Agenda.reporter` (« no brief is given for a program its skeleton.lock
  refuses ») et `soma holes`. On ne peut donc pas lire le brief du nouveau signal avant d'avoir accepté le changement.
- [+] Signalé tout seul, AVANT amend : `Agenda.annuler_rdv` calls `transition(id, "annule")` without saying from
  where, so it also takes `reporte -> annule` — outside its frame. Vrai impact, que j'aurais raté.
- [+] Signalé tout seul, mais seulement APRÈS amend : `warning: no signal moves `reporte -> confirme`` (j'avais
  oublié d'élargir `confirmer`), puis après élargissement `the frame says moves reporte -> confirme, the body never
  takes that edge`. Les deux m'ont mené droit au corps à changer.
- [A] D'où 3 `--amend` pour un seul changement (l'erreur de verrou masque les warnings de conception).
- Retrouvé à la main : rien ne dit qui DÉPEND de l'ensemble des états de `Agenda.rdv` (Rapports.dossier_visite,
  Notifications, Api, les tests renvoient/affichent l'état en String). Ici aucun ne branche dessus ; s'il y en avait
  un (`if statut == "confirme"`), aucun outil ne l'aurait montré. Les états sont des chaînes hors de leur cellule.
- Régressions : aucune ; verify et les 182 assertions sont restés verts ; +25 assertions.

### (b) plafond annuel par patient (Assurance), lu par Facturation
- Verrou : diff clair sur 2 cellules. `--amend` 1 fois.
- [+] Signalé tout seul, d'un seul `soma check` : les 2 `on …` dont la signature ne correspond plus (E0119, avec la
  ligne à écrire), l'appelant `Facturation.emettre` en arité fausse (E0041), et 4 warnings « the frame says … the
  body never … » qui sont exactement la liste des corps à compléter. C'est le meilleur moment de l'outil.
- [C] **Laissé passer : l'appel `Assurance.souscrire(…)` à 4 arguments dans un helper de `cell test`.** Ni
  `soma check app.cell`, ni `soma check test_parcours.cell` (« no errors »), ni la pré-vérification de `soma test`
  ne le voient ; à l'exécution : `ERROR: souscrire() expected 5 arguments, got 4 (raised at line 8)` puis 53
  assertions rouges en cascade (`undefined variable: cle`…). (repro/test_helper_arity)
- [C] Laissé passer par construction : les COMMENTAIRES ne sont pas dans le verrou. J'ai changé la spec de `emettre`
  (« engager(…, annee de la facture) ») sans que rien ne bouge ; or le commentaire EST le brief. Un changement de
  sens sans changement de cadre est invisible pour `--freeze`.
- Retrouvé à la main : la forme du record `contrats` (nouveau champ `plafond_annuel`) et `factures` (`annee`) — des
  `Map` non typés : aucun lecteur n'est signalé ; un ancien enregistrement donnerait `()` à l'exécution.
- Cadre trop large que j'ai dû accepter : `calls today` sur `preparer` (il n'y a pas de « reads clock »).

### (c) ordonnance délivrée seulement si la facture est payée
- Verrou : changer soma.toml suffit à déclencher E0121 (`- …requires = ["Consultations.terminee"]` /
  `+ …requires_all = […]`) : bien, la règle fait partie de la structure.
- [+] **Le cas d'école** : après amend, `soma check` vert, les 207 assertions vertes, et `soma verify` :
  `✗ before Ordonnances.delivree requires each of Consultations.terminee, Facturation.payee: a run of the program's
  handlers enters `delivree` without it (from Agenda=termine, Consultations=terminee, Ordonnances=signee, …,
  Facturation=brouillon, …)`. Les tests ne l'auraient pas vu. Il ne nomme pas le handler (`delivrer`), mais ici ça
  se devine.
- Fait à la main : ajouter `Facturation.exiger_payee` (même forme magique `require get_status(id) == …`), l'appeler
  dans `delivrer`, 2e `--amend`. Puis les tests : 10 assertions rouges, toutes légitimes (ordre du scénario).
- Régression métier que SEULS les tests ont montrée : une consultation sans acte (`RienAFacturer`) n'a jamais de
  facture, donc son ordonnance n'est plus jamais délivrable. Verify prouve la règle, pas qu'elle est vivable
  (il n'y a pas de « delivree reste atteignable depuis terminee sans facture » — et je ne sais pas l'écrire).

## Chiffres finaux
14 cellules, 115 signaux ; squelette 751 lignes (lib/ + app.cell), chair 715 lignes, tests 342 lignes ;
218 assertions (47 + 53 + 118), toutes vertes ; `soma check` ✓ (1 note) ; `soma verify` : VERIFY OK, 120 ✓
(dont 10 « vacuously true », 6 règles produit dont 4 inter-cellules), 18 ⚠ runtime-checked ; `--strict` échoue.
Carte : 469 lignes. Durées : non mesurées de façon fiable (les « ≈ min » de ce journal sont des estimations, les nombres d'essais sont exacts).
