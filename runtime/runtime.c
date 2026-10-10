#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include <stdint.h>
#include <string.h>

// TaggedVal tags
#define TAG_NUM     0
#define TAG_BOOL    1
#define TAG_CLOSURE 2
#define TAG_UNIT    3

typedef struct {
    int64_t tag;
    int64_t payload;
} Val;

typedef struct Closure {
    uint64_t fn_ptr;
    uint64_t n_caps;
    // captures follow: n_caps * 16 bytes (tag i64 + payload i64 each)
} Closure;

// Allocate a closure with fn_ptr and n_captures; captures are zeroed.
void* matte_alloc(uint64_t fn_ptr, uint64_t n_caps) {
    size_t sz = sizeof(Closure) + n_caps * 16;
    Closure* c = malloc(sz);
    if (!c) { fprintf(stderr, "matte: out of memory\n"); exit(1); }
    c->fn_ptr = fn_ptr;
    c->n_caps = n_caps;
    memset((char*)c + sizeof(Closure), 0, n_caps * 16);
    return c;
}

static double bits_to_f64(int64_t b) {
    double d; memcpy(&d, &b, 8); return d;
}
static int64_t f64_to_bits(double d) {
    int64_t b; memcpy(&b, &d, 8); return b;
}

void matte_print(int64_t tag, int64_t payload) {
    if (tag == TAG_NUM) {
        double d = bits_to_f64(payload);
        if (d == (long long)d) printf("%lld\n", (long long)d);
        else                  printf("%g\n", d);
    } else if (tag == TAG_BOOL) {
        printf("%s\n", payload ? "true" : "false");
    } else {
        fprintf(stderr, "matte: cannot print value of this type\n");
        exit(1);
    }
}

int64_t matte_pow(int64_t xb, int64_t yb) {
    return f64_to_bits(pow(bits_to_f64(xb), bits_to_f64(yb)));
}

int64_t matte_fmod(int64_t xb, int64_t yb) {
    return f64_to_bits(fmod(bits_to_f64(xb), bits_to_f64(yb)));
}

int64_t matte_fact(int64_t xb) {
    double x = bits_to_f64(xb);
    long long n = (long long)x;
    double r = 1.0;
    for (long long i = 2; i <= n; i++) r *= (double)i;
    return f64_to_bits(r);
}

extern void matte_main(void);

int main(void) {
    matte_main();
    return 0;
}
