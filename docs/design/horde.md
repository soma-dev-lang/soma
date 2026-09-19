# Hordes d'agents — conception (phase 0)

Objectif : qu'un programme Soma fasse travailler **10 000 agents** sur un
sujet, sans perdre ce qui fait Soma : atomicité des écritures, invariants,
coût **prouvé**, reprise après crash, tests hors ligne.

## 1. Deux cas pilotes

| | A. Fan-out vérifié | B. Simulation de population |
|---|---|---|
| Exemple | auditer 10 000 documents / fichiers, un agent par élément, puis synthèse | 10 000 consommateurs réagissent à une hausse de prix, sur 20 tours |
| Forme | `map` puis `reduce` ; tâches indépendantes | tours synchronisés ; chaque agent lit l'état du monde, agit ; l'état évolue |
| Ce qui compte | débit, coût total borné, résultats exactement-une-fois, reprise | reproductibilité, temps simulé, coût par tour, ordre déterministe des effets |
| Références | Anthropic research system (orchestrateur + sous-agents), Kimi Agent Swarm, audit « un agent par fichier » | AgentSociety (10k agents), « Generative Agent Simulations of 1,000 People » |

Le cas A est construit en premier ; le cas B réutilise tout (tâches, pool,
budget) et ajoute les tours.

## 2. Ce qui bloque dans le runtime actuel

1. **Tout passe par un verrou unique.** `Interpreter::atomically`
   (`interpreter/mod.rs`) prend `HANDLER_LOCK` puis ouvre une transaction
   SQLite `BEGIN IMMEDIATE` pour toute la durée du handler. Un `think()`
   (appel HTTP de plusieurs secondes) s'exécute **dans** cette transaction :
   10 000 appels de 5 s = ~14 h, séquentiels.
2. **Un agent = une cellule.** `cell agent X` existe une fois ; sa
   conversation est `agent_conversations[cell]`. Il n'y a pas d'instances.
3. **Budgets par invocation.** `set_budget` / `tokens_used()` vivent sur
   l'interpréteur d'une requête ou d'un tick ; rien ne borne la dépense
   d'un ensemble d'agents.
4. **La preuve de coût est par handler** (`cost { tokens: N }`), pas par
   lot.
5. **Aucune primitive de parallélisme** (seul `par_read_files` existe).

## 3. Principe : séparer « penser » et « écrire »

Un appel LLM ne doit jamais tenir le verrou. Une **tâche d'agent** est donc
une suite d'**étapes** :

```
étape 1 (transaction courte) : lire l'entrée, l'état utile
appel LLM (hors verrou, hors transaction, en parallèle avec d'autres tâches)
étape 2 (transaction courte) : valider la réponse, écrire le résultat
```

Chaque étape reste atomique et soumise aux invariants ; ce qui change :
**une tâche entière n'est plus atomique** (l'étape 1 a pu committer). Pour
ne pas surprendre, c'est une forme de handler distincte, marquée `[task]`
— les handlers ordinaires gardent exactement leur sémantique actuelle.

Dans un handler `[task]`, `think()` est un **point de commit** : les
écritures faites avant sont committées, le verrou est rendu pendant l'appel,
puis repris. Le vérificateur le sait (voir §6).

## 4. Proposition de langage (cas A)

```soma
cell type Doc     { variants { Doc { id: String, text: String } } }
cell type Verdict { variants { Verdict { id: String, risk: Int, note: String } } }

cell agent Reviewer {
    cost { tokens: 2000 }                      // par tâche — prouvé comme aujourd'hui
    on review(d: Doc) [task] {
        let r = think_json("Évalue le risque de: {d.text}", map("max_tokens", 1500))
        require r.risk >= 0 && r.risk <= 10 else BadRisk
        return Verdict { id: d.id, risk: r.risk, note: r.note }
    }
}

cell Audit {
    memory {
        verdicts: Map<String, Verdict> [persistent, immutable]   // exactement une fois par doc
    }
    on start_audit(docs: List<Doc>) {
        // non bloquant : renvoie l'id de la horde tout de suite
        return horde(Reviewer.review, docs, map(
            "concurrency", 200,               // appels LLM en vol au maximum
            "budget_tokens", 20000000,        // plafond DUR pour toute la horde
            "max_attempts", 3,
            "on_result", "_store",            // appelé (atomiquement) pour chaque résultat
            "on_done", "_summarize"))
    }
    on _store(v: Verdict) { verdicts.set(v.id, v) }
    on _summarize(h: String) { … horde_stats(h) … }        // agrégation / synthèse
}
```

Primitives :

| Primitive | Rôle |
|---|---|
| `horde(handler, inputs, opts)` | crée N tâches persistées, renvoie un id ; ne bloque pas |
| `horde_status(id)` | `{queued, running, done, failed, tokens, cost}` |
| `horde_cancel(id)` | arrête (budget, quorum atteint, décision humaine) |
| `on_result` / `on_done` | handlers ordinaires (atomiques) appelés par le runtime |
| `vote(handler, input, k)` | k agents sur le même élément, majorité / désaccord (cas « débat ») |

## 5. Runtime

- **File de tâches persistée** (tables `__horde`, `__task` dans `soma.db`) :
  id, horde, entrée, état (`queued → running → done | failed`), tentatives,
  résultat, tokens. Un crash remet les `running` en `queued` ; comme le
  résultat est écrit une seule fois (`[immutable]`), la reprise est
  exactement-une-fois côté données.
- **Pool asynchrone** : les appels LLM partent d'un pool borné
  (`concurrency`), pas d'un thread par agent. Les étapes transactionnelles
  repassent par `atomically`, donc restent sérialisées et courtes.
- **Limiteur de débit fournisseur** : seau à jetons RPM / TPM configuré dans
  `soma.toml [agent]` ; les 429 font reculer le seau, pas l'agent.
- **Budget par réservation** : avant chaque appel, le runtime **réserve**
  `taille du prompt + max_tokens` sur le compteur de la horde ; il règle la
  dépense réelle après. Le plafond est donc **dur** (jamais dépassé), pas
  « le suivant échoue » comme `set_budget` aujourd'hui.
- **Journal des réponses LLM** par tâche : replay déterministe, audit.
- **Arrêt** : budget épuisé, `horde_cancel`, date limite ; les tâches en vol
  finissent ou sont abandonnées selon l'option.

## 6. Ce que le vérificateur prouve

- **Coût de la horde** : `≤ min(budget_tokens, n × borne_par_tâche × max_attempts) + borne(on_result, on_done)`.
  La borne par tâche est la preuve `cost` existante ; `n` est la taille des
  entrées (ou `max_agents` déclaré). Un `budget_tokens` littéral rend la
  borne vraie même quand `n` est inconnu.
- **Terminaison** : entrées finies, tentatives bornées, chaque tâche termine
  (analyse actuelle) ⇒ la horde termine.
- **Handlers `[task]`** : chaque étape est vérifiée comme un handler ; un
  invariant dont la preuve dépend d'un `require` lu **avant** un `think()`
  n'est pas prouvé pour une écriture faite **après** (l'état a pu changer
  pendant l'appel) — même règle que celle du cycle 64.
- **Exactement-une-fois** : `[immutable]` sur le slot de résultats.

## 7. Cas B : simulation par tours

```soma
cell agent Consumer {
    memory { mood: Map<String, Int> [persistent] }        // par instance (clé = id d'agent)
    on act(world: Map) [task] { … think(…) … return map("buy", b) }
}
cell Market {
    on step(round: Int) {
        return horde(Consumer.act, population(), map("snapshot", world(), "apply", "_apply", "concurrency", 500))
    }
}
```

- **Instances** : `population()` = ids ; la mémoire d'un `cell agent` est
  indexée par id d'instance ; la conversation aussi.
- **Instantané** : chaque agent d'un tour lit le même état (`snapshot`),
  puis les effets sont appliqués **en fin de tour dans l'ordre des ids** —
  résultat indépendant de l'ordre d'arrivée des réponses.
- **Reproductible** : graine + réponses LLM journalisées = même simulation.

## 8. Tests hors ligne

- `mock think` scripté par handler et par motif (`mock think Reviewer.review "…"`),
  une latence simulée, un mode « 10 000 agents en mémoire » en quelques secondes.
- `soma test` exécute une horde de façon synchrone avec les mocks.

## 9. Observabilité et sécurité

- Tableau de bord de horde (états, tokens, coût, erreurs), événements SSE de
  progression, trace par tâche.
- La sortie d'un agent est une **donnée**, jamais une instruction pour un
  autre agent (injection qui se propage) ; capabilities d'outils par tâche ;
  budget qui ne peut que baisser depuis un outil (déjà en place).

## 10. Phases et critères d'acceptation

| Phase | Livrable | Accepté quand |
|---|---|---|
| 1 ✅ | handlers `[task]` : `think()` hors verrou, étapes transactionnelles (fait : 200 × 2 s en 9,3 s) | 200 `think()` mockés de 2 s en parallèle finissent en < 10 s ; aucune écriture perdue ; les tests existants passent |
| 2 ✅ | `horde` / `horde_status` / `horde_cancel`, file persistée, pool, limiteur RPM/TPM (fait : 10 000 × 2 s, concurrence 500, en 41 s ; `kill -9` après 2 500 puis reprise : 10 000 résultats dans une Map `[immutable]`, chacun une fois) | 10 000 tâches mockées (latence 2 s, concurrence 500) en < 2 min ; `kill -9` au milieu puis reprise : chaque résultat écrit une fois |
| 3 ✅ | budget par réservation, preuve de coût de horde (fait : 1 000 tâches, 200 en vol, budget 20 000 → 19 896 dépensés, jamais au-delà ; `verify` affiche la borne ; un fournisseur qui ignore `max_tokens` est détecté et facturé, le plafond suppose qu'il le respecte) | le plafond n'est jamais dépassé, même avec un fournisseur qui ignore `max_tokens` ; `verify` affiche la borne |
| 4 ✅ | `vote`, `snapshot`/tours, instances d'agents (cas B) (fait : `snapshot` + `apply` en ordre d'entrée + `seed` + `instance` ; 10 000 agents × 20 tours en 46 s, deux exécutions identiques) | simulation de 10 000 agents × 20 tours reproductible à l'identique avec la même graine |
| 5 | tableau de bord, mocks par motif, doc + exemples du corpus | un agent externe écrit un audit de 10 000 documents depuis la doc seule |

## 11. Questions ouvertes

1. Nom et forme : `[task]` sur le handler, ou un type de cellule `cell worker` ?
2. `on_result` appelé par tâche (simple, sérialisé) ou par lots (débit) ?
3. Modèles par niveau : le chef en gros modèle, les travailleurs en petit
   (`[model: …]` existe déjà par cellule) — suffisant ?
4. Distribution multi-machines : hors périmètre tant que le débit est limité
   par le fournisseur, pas par la machine.
