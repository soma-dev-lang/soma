CC      = clang
CFLAGS  = -O3 -march=native -std=c11 -Wall
LDLIBS  = -lm

GMP_PREFIX = /opt/homebrew/opt/gmp
GMP_CFLAGS = -I$(GMP_PREFIX)/include
GMP_LDFLAGS = -L$(GMP_PREFIX)/lib -lgmp

BIN = nbody spectral_norm mandelbrot fannkuch pidigits

all: $(BIN)

nbody: nbody.c
	$(CC) $(CFLAGS) $< -o $@ $(LDLIBS)

spectral_norm: spectral_norm.c
	$(CC) $(CFLAGS) $< -o $@ $(LDLIBS)

mandelbrot: mandelbrot.c
	$(CC) $(CFLAGS) $< -o $@ $(LDLIBS)

fannkuch: fannkuch.c
	$(CC) $(CFLAGS) $< -o $@ $(LDLIBS)

pidigits: pidigits.c
	$(CC) $(CFLAGS) $(GMP_CFLAGS) $< -o $@ $(GMP_LDFLAGS) $(LDLIBS)

clean:
	rm -f $(BIN)

.PHONY: all clean
