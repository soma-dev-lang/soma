# Build the mechanized soundness proof for the Soma CTL model checker.
#
#   make             # compile Soma_CTL.v
#   make check       # compile + verify zero axioms / zero Admitted
#   make clean       # remove build artifacts
#
# Requires: Rocq Prover 9.1+ (formerly Coq). Install with `brew install rocq`
# on macOS or your distribution's package manager.

.PHONY: all check clean

all: Soma_CTL.vo Soma_Budget.vo Soma_Isolation.vo Soma_Abstraction.vo Soma_BudgetOps.vo

Soma_CTL.vo: Soma_CTL.v
	coqc Soma_CTL.v

Soma_Budget.vo: Soma_Budget.v
	coqc Soma_Budget.v

Soma_Isolation.vo: Soma_Isolation.v
	coqc Soma_Isolation.v

Soma_Abstraction.vo: Soma_Abstraction.v
	coqc Soma_Abstraction.v

Soma_BudgetOps.vo: Soma_BudgetOps.v
	coqc Soma_BudgetOps.v

check: all
	@echo
	@echo "==== Soma_CTL: zero axioms / zero Admitted ===="
	@printf 'Require Import Soma_CTL.\nPrint Assumptions acyclic_walk_bounded.\nPrint Assumptions walk_too_long_must_repeat.\nPrint Assumptions bound_at_least_states_is_sufficient.\nPrint Assumptions walk_g4.\nPrint Assumptions any_5_walk_in_g4_must_repeat.\n' > _check.v
	@coqc _check.v 2>&1 | grep -E "Closed under|Axioms|axiom" || true
	@rm -f _check.v _check.vo _check.glob _check.vok _check.vos
	@echo
	@echo "==== Soma_Budget: zero axioms / zero Admitted ===="
	@printf 'Require Import Soma_Budget.\nPrint Assumptions cplus_monotone.\nPrint Assumptions cmax_lub.\nPrint Assumptions ctimes_distributes_iter.\nPrint Assumptions cost_composition_sound.\nPrint Assumptions cell_peak_sound.\nPrint Assumptions handler_a_sound.\n' > _check.v
	@coqc _check.v 2>&1 | grep -E "Closed under|Axioms|axiom" || true
	@rm -f _check.v _check.vo _check.glob _check.vok _check.vos
	@echo
	@echo "==== Soma_Isolation: zero axioms / zero Admitted ===="
	@printf 'Require Import Soma_Isolation.\nPrint Assumptions think_isolation_safety.\nPrint Assumptions safety_oracle_independent.\n' > _check.v
	@coqc _check.v 2>&1 | grep -E "Closed under|Axioms|axiom" || true
	@rm -f _check.v _check.vo _check.glob _check.vok _check.vos
	@echo
	@echo "==== Soma_Abstraction: zero axioms / zero Admitted ===="
	@printf 'Require Import Soma_Abstraction.\nPrint Assumptions trace_all_safe.\nPrint Assumptions abstraction_transfers_safety.\n' > _check.v
	@coqc _check.v 2>&1 | grep -E "Closed under|Axioms|axiom" || true
	@rm -f _check.v _check.vo _check.glob _check.vok _check.vos
	@echo
	@echo "==== Soma_BudgetOps: zero axioms / zero Admitted ===="
	@printf 'Require Import Soma_BudgetOps.\nPrint Assumptions handler_alloc_bounded.\nPrint Assumptions loop_alloc_bounded.\nPrint Assumptions branch_alloc_bounded.\n' > _check.v
	@coqc _check.v 2>&1 | grep -E "Closed under|Axioms|axiom" || true
	@rm -f _check.v _check.vo _check.glob _check.vok _check.vos
	@echo
	@echo "If every line above says 'Closed under the global context',"
	@echo "the proofs depend on no axioms beyond Coq's stdlib."

clean:
	rm -f *.vo *.vok *.vos *.glob *.aux _check.v
