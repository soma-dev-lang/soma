# Build the mechanized soundness proof for the Soma CTL model checker.
#
#   make             # compile Soma_CTL.v
#   make check       # compile + verify zero axioms / zero Admitted
#   make clean       # remove build artifacts
#
# Requires: Rocq Prover 9.1+ (formerly Coq). Install with `brew install rocq`
# on macOS or your distribution's package manager.

.PHONY: all check clean

all: Soma_CTL.vo

Soma_CTL.vo: Soma_CTL.v
	coqc Soma_CTL.v

check: Soma_CTL.vo
	@echo
	@echo "==== Verifying zero axioms / zero Admitted ===="
	@printf 'Require Import Soma_CTL.\n\
	Print Assumptions acyclic_walk_bounded.\n\
	Print Assumptions walk_too_long_must_repeat.\n\
	Print Assumptions bound_at_least_states_is_sufficient.\n\
	Print Assumptions walk_g4.\n\
	Print Assumptions any_5_walk_in_g4_must_repeat.\n' > _check.v
	@coqc _check.v 2>&1 | grep -E "Closed under|Axioms|axiom" || true
	@rm -f _check.v _check.vo _check.glob _check.vok _check.vos
	@echo
	@echo "If every line above says 'Closed under the global context',"
	@echo "the proof depends on no axioms beyond Coq's stdlib."

clean:
	rm -f *.vo *.vok *.vos *.glob *.aux _check.v
