/* rgoc 的 native runtime（M7；freestanding，**不依赖 libc**）。
 *
 * 编译：clang -ffreestanding -fno-builtin -nostdlib -O2 -c rt.c -o rt.o
 * 链接：clang -nostartfiles -Wl,-s -o prog prog.s rt.o
 *
 * # 为什么 -fno-builtin 是必需的
 *
 * 下面自己实现的 `rt_memcpy` / `rt_memset` 如果被 clang 认成「内建 memcpy」，
 * 它会把循环优化成**一次 memcpy 调用** —— 也就是调用它自己。链接能过，
 * 运行期无限递归。`-fno-builtin` 关掉这个替换。
 *
 * # 为什么没有 libc
 *
 * M0 的 spike 已经证过：`svc #0` 直达内核，一个 libc 符号都不需要（`C5` §2.5）。
 * 这条路线在 M7 继续 —— runtime 只有系统调用与自己的循环。
 *
 * # 没有实现的入口**故意不定义**
 *
 * 例如 `rt_print_f64`（最短往返浮点格式化，Task 43）。没实现就不写一个「大概对」的版本：
 * 定义了它，用到的地方会**静默打错**；不定义，链接期直接 `undefined reference`。
 */

typedef unsigned long u64;
typedef long i64;
typedef unsigned int u32;
typedef unsigned char u8;

#define SYS_write 64
#define SYS_exit_group 94

#define FD_STDOUT 1
#define FD_STDERR 2

/* 三个参数的系统调用（write / exit_group 都够用）。 */
static i64 sys3(i64 n, i64 a, i64 b, i64 c) {
    register i64 x8 __asm__("x8") = n;
    register i64 x0 __asm__("x0") = a;
    register i64 x1 __asm__("x1") = b;
    register i64 x2 __asm__("x2") = c;
    __asm__ volatile("svc #0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2) : "memory");
    return x0;
}

void rt_exit(i64 code) {
    sys3(SYS_exit_group, code, 0, 0);
    for (;;) {
    }
}

/* 短写必须循环（`C5` §2.3 的已知缺口，这里补上）。 */
static void write_all(i64 fd, const char *p, u64 n) {
    while (n > 0) {
        i64 w = sys3(SYS_write, fd, (i64)p, (i64)n);
        if (w <= 0) {
            return;
        }
        p += (u64)w;
        n -= (u64)w;
    }
}

void rt_write(i64 fd, const char *p, u64 n) { write_all(fd, p, n); }

/* ── memcpy / memset（必须自己实现：freestanding 下 clang 仍可能发这两个调用）── */

void *rt_memcpy(void *dst, const void *src, u64 n) {
    u8 *d = (u8 *)dst;
    const u8 *s = (const u8 *)src;
    for (u64 i = 0; i < n; i++) {
        d[i] = s[i];
    }
    return dst;
}

void *rt_memset(void *dst, int c, u64 n) {
    u8 *d = (u8 *)dst;
    for (u64 i = 0; i < n; i++) {
        d[i] = (u8)c;
    }
    return dst;
}

/* ── 分配：静态 arena + bump（`C5` §7.2 / `D-M7-7`）── */

#define ARENA_SIZE (1024UL * 1024UL)

static u8 arena[ARENA_SIZE];
static u64 arena_used;

/* 零大小对象的固定地址（`zerosize.go` 要求 `new([0]int)` 两次相等）。 */
static u8 zerobase;

void *rt_arena_alloc(u64 size, u64 align) {
    if (size == 0) {
        return &zerobase;
    }
    if (align == 0) {
        align = 8;
    }
    u64 p = (arena_used + align - 1) & ~(align - 1);
    if (p + size > ARENA_SIZE) {
        /* 没有实现「panic 文案」的依赖循环问题：直接写死一行。 */
        const char *m = "panic: out of arena\n";
        u64 n = 0;
        while (m[n] != 0) {
            n++;
        }
        write_all(FD_STDERR, m, n);
        rt_exit(2);
    }
    void *r = (void *)(arena + p);
    arena_used = p + size;
    rt_memset(r, 0, size);
    return r;
}

/* ── 输出 ── */

void rt_print_space(void) { write_all(FD_STDOUT, " ", 1); }

void rt_print_nl(void) { write_all(FD_STDOUT, "\n", 1); }

void rt_print_str(const char *p, u64 n) { write_all(FD_STDOUT, p, n); }

void rt_print_bool(i64 b) {
    if (b != 0) {
        write_all(FD_STDOUT, "true", 4);
    } else {
        write_all(FD_STDOUT, "false", 5);
    }
}

/* 无符号十进制（倒着填缓冲区）。 */
static u64 fmt_u64(u64 v, char *buf) {
    char tmp[20];
    u64 n = 0;
    if (v == 0) {
        buf[0] = '0';
        return 1;
    }
    while (v > 0) {
        tmp[n++] = (char)('0' + (v % 10));
        v /= 10;
    }
    for (u64 i = 0; i < n; i++) {
        buf[i] = tmp[n - 1 - i];
    }
    return n;
}

void rt_print_u64(u64 v) {
    char buf[20];
    u64 n = fmt_u64(v, buf);
    write_all(FD_STDOUT, buf, n);
}

void rt_print_i64(i64 v) {
    char buf[21];
    u64 n = 0;
    if (v < 0) {
        buf[0] = '-';
        n = 1;
        /* `i64::MIN` 取负会溢出 ⇒ 先转 u64 再取负。 */
        n += fmt_u64((u64)0 - (u64)v, buf + 1);
    } else {
        n = fmt_u64((u64)v, buf);
    }
    write_all(FD_STDOUT, buf, n);
}

/* ── panic（`C9` §4.5：一行到 stderr + 退出码 2）── */

static void panic_prefix(void) { write_all(FD_STDERR, "panic: ", 7); }

void rt_panic_str(const char *p, u64 n) {
    panic_prefix();
    write_all(FD_STDERR, p, n);
    write_all(FD_STDERR, "\n", 1);
    rt_exit(2);
}

void rt_panic_i64(i64 v) {
    char buf[21];
    u64 n = 0;
    if (v < 0) {
        buf[0] = '-';
        n = 1 + fmt_u64((u64)0 - (u64)v, buf + 1);
    } else {
        n = fmt_u64((u64)v, buf);
    }
    panic_prefix();
    write_all(FD_STDERR, buf, n);
    write_all(FD_STDERR, "\n", 1);
    rt_exit(2);
}

/* 三个检查点的出口（`M7-design.md` §3.9）。文案与 Go 的对应 panic 一致。 */
void rt_panic_div_zero(void) {
    const char *m = "panic: runtime error: integer divide by zero\n";
    u64 n = 0;
    while (m[n] != 0) {
        n++;
    }
    write_all(FD_STDERR, m, n);
    rt_exit(2);
}

void rt_panic_nil(void) {
    const char *m =
        "panic: runtime error: invalid memory address or nil pointer dereference\n";
    u64 n = 0;
    while (m[n] != 0) {
        n++;
    }
    write_all(FD_STDERR, m, n);
    rt_exit(2);
}

/* ════════════════════════════════════════════════════════════════════
 * 浮点格式化：`strconv.FormatFloat(f, 'g', -1, bits)` 的等价物
 * （`C9` §4.3/§4.4；`M7-design.md` `D-M7-10`）
 *
 * # 为什么自己算，而不是「差不多就行」
 *
 * 判据是**最短往返**：打印出来的十进制串读回去必须还是同一个浮点数，且位数最少。
 * 「固定 17 位再裁尾零」不是最短往返（`0.1` 会打成 `0.10000000000000001`），
 * 而语料 42/46 是空输出 —— 打错根本抓不到，只会在用户程序里爆。
 *
 * # 算法（Steele & White 的 free-format，即 Dragon4 的「最短」模式）
 *
 * 1. 把 |v| 写成 `m × 2^e`（精确整数）；
 * 2. 用大整数求出它的**精确十进制展开**（`m << e` 或 `m × 5^-e`，都是精确的）；
 * 3. 从 1 位到 17 位，把精确展开**四舍六入五成双**到 N 位，问：这个十进制数
 *    是否落在 `v` 的舍入区间 `((2m-1)·2^(e-1), (2m+1)·2^(e-1))` 里？
 *    第一个落在里面的 N 就是最短位数。
 *
 * 区间比较全用大整数做（乘以 10 的幂 / 2 的幂后比大小），没有浮点参与 ——
 * 一旦用浮点去判「够不够近」，就回到「差不多」了。
 *
 * # 幂次边界（`frac == 0` 且不是最小指数）
 *
 * `v` 恰是 2 的幂时，**下界只有半个 ulp 的一半**（`4m-1` 而不是 `2m-1`）。
 * 漏了这一条，`1.0`/`2.0`/`1024.0` 这类值会多打一位。
 * ════════════════════════════════════════════════════════════════════ */

#define BN_LIMBS 96 /* 3072 位：5^1074 ≈ 2494 位，够用 */

typedef struct {
    u32 d[BN_LIMBS]; /* 小端：d[0] 是最低 32 位 */
    int n;           /* 有效 limb 数 */
} BN;

static void bn_zero(BN *a) {
    for (int i = 0; i < BN_LIMBS; i++) {
        a->d[i] = 0;
    }
    a->n = 0;
}

static void bn_from_u64(BN *a, u64 v) {
    bn_zero(a);
    a->d[0] = (u32)v;
    a->d[1] = (u32)(v >> 32);
    a->n = a->d[1] ? 2 : (a->d[0] ? 1 : 0);
}

static void bn_trim(BN *a) {
    while (a->n > 0 && a->d[a->n - 1] == 0) {
        a->n--;
    }
}

static void bn_shl(BN *a, u32 bits) {
    if (a->n == 0 || bits == 0) {
        return;
    }
    u32 limbs = bits / 32;
    u32 rem = bits % 32;
    if (limbs) {
        for (int i = a->n - 1; i >= 0; i--) {
            a->d[i + (int)limbs] = a->d[i];
        }
        for (u32 i = 0; i < limbs; i++) {
            a->d[i] = 0;
        }
        a->n += (int)limbs;
    }
    if (rem) {
        u32 carry = 0;
        for (int i = (int)limbs; i < a->n; i++) {
            u32 v = a->d[i];
            a->d[i] = (v << rem) | carry;
            carry = v >> (32 - rem);
        }
        if (carry) {
            a->d[a->n++] = carry;
        }
    }
}

static void bn_mul_u32(BN *a, u32 m) {
    u64 carry = 0;
    for (int i = 0; i < a->n; i++) {
        u64 v = (u64)a->d[i] * m + carry;
        a->d[i] = (u32)v;
        carry = v >> 32;
    }
    while (carry) {
        a->d[a->n++] = (u32)carry;
        carry >>= 32;
    }
}

/* a ×= 10^k（k ≤ 40 够用：区间比较最多要 10^17 量级） */
static void bn_mul_pow10(BN *a, u32 k) {
    while (k >= 9) {
        bn_mul_u32(a, 1000000000u);
        k -= 9;
    }
    static const u32 p10[9] = {1u, 10u, 100u, 1000u, 10000u, 100000u, 1000000u, 10000000u, 100000000u};
    if (k) {
        bn_mul_u32(a, p10[k]);
    }
}

static int bn_cmp(const BN *a, const BN *b) {
    if (a->n != b->n) {
        return a->n < b->n ? -1 : 1;
    }
    for (int i = a->n - 1; i >= 0; i--) {
        if (a->d[i] != b->d[i]) {
            return a->d[i] < b->d[i] ? -1 : 1;
        }
    }
    return 0;
}

/* a 除以 small，返回余数 */
static u32 bn_divmod_u32(BN *a, u32 m) {
    u64 rem = 0;
    for (int i = a->n - 1; i >= 0; i--) {
        u64 cur = (rem << 32) | a->d[i];
        a->d[i] = (u32)(cur / m);
        rem = cur % m;
    }
    bn_trim(a);
    return (u32)rem;
}

/* 十进制展开：把 a 写成 10 进制数字串（高位在前），返回长度 */
static int bn_to_dec(BN a, char *out) {
    char tmp[800];
    int n = 0;
    if (a.n == 0) {
        out[0] = '0';
        return 1;
    }
    while (a.n > 0) {
        u32 r = bn_divmod_u32(&a, 1000000000u);
        for (int i = 0; i < 9; i++) {
            tmp[n++] = (char)('0' + r % 10);
            r /= 10;
        }
    }
    while (n > 1 && tmp[n - 1] == '0') {
        n--;
    }
    for (int i = 0; i < n; i++) {
        out[i] = tmp[n - 1 - i];
    }
    return n;
}

/* 把 `C × 10^E` 与 `(2m ± 1) × 2^(e-1)` 比大小：返回 -1/0/1。
 * `sign` = +1 用 (2m+1)（上界），-1 用 (2m-1)（下界）。 */
static int cmp_cand_bound(u64 C, int E, u64 m, int e, int sign) {
    BN lhs;
    bn_from_u64(&lhs, C);
    BN rhs;
    bn_from_u64(&rhs, 2 * m + (u64)sign);
    /* rhs = (2m±1) × 2^(e-1) */
    if (e >= 1) {
        bn_shl(&rhs, (u32)(e - 1));
    } else {
        /* e-1 < 0 ⇒ 两边同乘 2^(1-e) */
        bn_shl(&lhs, (u32)(1 - e));
    }
    if (E >= 0) {
        bn_mul_pow10(&lhs, (u32)E);
    } else {
        bn_mul_pow10(&rhs, (u32)(-E));
    }
    return bn_cmp(&lhs, &rhs);
}

/* 把十进制数字串 `d`（长 n）舍入到 `keep` 位（四舍六入五成双），
 * 结果写回 `d`（长 *out_n），指数调整写进 `*exp10`（值 = d × 10^exp10）。 */
static void round_digits(char *d, int n, int keep, int *out_n, int *exp10) {
    int shift = n - keep; /* 去掉多少位 */
    if (shift <= 0) {
        *out_n = n;
        return;
    }
    /* ⚠️ 丢掉 shift 位 ⇒ 值变小了 10^shift 倍，指数必须补回来。
     * 漏了这一行，`0.0001` 会打成它的完整精确展开（`1e-4` 永远过不了区间判定）。 */
    *exp10 += shift;
    int round_up = 0;
    char first = d[keep];
    if (first > '5') {
        round_up = 1;
    } else if (first == '5') {
        /* 后面还有非零 ⇒ 大于一半；否则正好一半 ⇒ 看奇偶 */
        int nonzero = 0;
        for (int i = keep + 1; i < n; i++) {
            if (d[i] != '0') {
                nonzero = 1;
                break;
            }
        }
        if (nonzero) {
            round_up = 1;
        } else {
            round_up = (d[keep - 1] - '0') % 2; /* 半到偶 */
        }
    }
    int len = keep;
    if (round_up) {
        int i = keep - 1;
        while (i >= 0) {
            if (d[i] != '9') {
                d[i]++;
                break;
            }
            d[i] = '0';
            i--;
        }
        if (i < 0) {
            /* 全 9 ⇒ 进位成 1 后面跟 0 */
            for (int k = keep - 1; k > 0; k--) {
                d[k] = '0';
            }
            d[0] = '1';
            *exp10 += 1;
        }
    }
    *out_n = len;
}

/* 去掉尾零（最短表示不带无意义的 0） */
static void trim_zeros(char *d, int *n) {
    while (*n > 1 && d[*n - 1] == '0') {
        (*n)--;
    }
}

/* `%g` 的指数判定：`exp < -4 || exp >= 6` ⇒ 指数形式，指数至少两位。 */
static int emit_g(const char *digits, int nd, int sci_exp, int neg, char *out) {
    int k = 0;
    if (neg) {
        out[k++] = '-';
    }
    if (sci_exp < -4 || sci_exp >= 6) {
        out[k++] = digits[0];
        if (nd > 1) {
            out[k++] = '.';
            for (int i = 1; i < nd; i++) {
                out[k++] = digits[i];
            }
        }
        out[k++] = 'e';
        int e = sci_exp;
        if (e >= 0) {
            out[k++] = '+';
        } else {
            out[k++] = '-';
            e = -e;
        }
        if (e < 10) {
            out[k++] = '0';
        }
        char eb[4];
        int en = 0;
        if (e == 0) {
            eb[en++] = '0';
        }
        while (e > 0) {
            eb[en++] = (char)('0' + e % 10);
            e /= 10;
        }
        for (int i = en - 1; i >= 0; i--) {
            out[k++] = eb[i];
        }
    } else if (sci_exp >= 0) {
        int ip = sci_exp + 1; /* 整数部分位数 */
        if (nd <= ip) {
            for (int i = 0; i < nd; i++) {
                out[k++] = digits[i];
            }
            for (int i = nd; i < ip; i++) {
                out[k++] = '0';
            }
        } else {
            for (int i = 0; i < ip; i++) {
                out[k++] = digits[i];
            }
            out[k++] = '.';
            for (int i = ip; i < nd; i++) {
                out[k++] = digits[i];
            }
        }
    } else {
        out[k++] = '0';
        out[k++] = '.';
        for (int i = 0; i < -sci_exp - 1; i++) {
            out[k++] = '0';
        }
        for (int i = 0; i < nd; i++) {
            out[k++] = digits[i];
        }
    }
    return k;
}

/* 核心：`m × 2^e`（精确）→ 最短十进制串。返回长度，写进 out（≥ 32 字节）。 */
static int fmt_bits(u64 m, int e, int neg, char *out) {
    if (m == 0) {
        out[0] = '0';
        return 1;
    }
    /* ① 精确十进制展开 */
    char dec[800];
    int exp10;
    BN big;
    bn_from_u64(&big, m);
    if (e >= 0) {
        bn_shl(&big, (u32)e);
        exp10 = 0;
    } else {
        int k = -e;
        for (int i = 0; i < k; i++) {
            bn_mul_u32(&big, 5);
        }
        exp10 = -k;
    }
    int nd = bn_to_dec(big, dec);

    /* ② 从 1 位到 17 位试最短 */
    for (int keep = 1; keep <= 17 && keep <= nd; keep++) {
        char cand[800];
        for (int i = 0; i < nd; i++) {
            cand[i] = dec[i];
        }
        int cn = nd;
        int ce = exp10;
        round_digits(cand, nd, keep, &cn, &ce);
        /* cand[0..cn) 是数字串，值 = cand × 10^ce */
        u64 C = 0;
        for (int i = 0; i < cn; i++) {
            C = C * 10 + (u64)(cand[i] - '0');
        }
        /* 落在舍入区间里？ */
        int below_hi = cmp_cand_bound(C, ce, m, e, +1) < 0;
        int above_lo;
        if ((m & ((1ULL << 52) - 1)) == 0 && m != 0 && e > -1074 && (m >> 52) == 1) {
            /* 2 的幂：下界是 (4m-1)·2^(e-2) */
            above_lo = cmp_cand_bound(C, ce, 2 * m, e - 1, -1) > 0;
        } else {
            above_lo = cmp_cand_bound(C, ce, m, e, -1) > 0;
        }
        if (below_hi && above_lo) {
            int dn = cn;
            trim_zeros(cand, &dn);
            int sci = ce + cn - 1;
            return emit_g(cand, dn, sci, neg, out);
        }
    }
    /* 17 位还不收敛（理论上不会）：退回全精度 */
    {
        char cand[800];
        for (int i = 0; i < nd; i++) {
            cand[i] = dec[i];
        }
        int dn = nd;
        trim_zeros(cand, &dn);
        int sci = exp10 + nd - 1;
        return emit_g(cand, dn, sci, neg, out);
    }
}

static int is_nan_f64(u64 bits) { return ((bits >> 52) & 0x7ff) == 0x7ff && (bits & ((1ULL << 52) - 1)) != 0; }
static int is_inf_f64(u64 bits) { return ((bits >> 52) & 0x7ff) == 0x7ff && (bits & ((1ULL << 52) - 1)) == 0; }

/* `println` 一个浮点（`x0` = is32）。 */
void rt_print_f64(double v, i64 is32) {
    u64 bits;
    /* 用位模式取值，避免任何隐式转换 */
    if (is32) {
        /* `v` 里放的是 float32 的值（低 32 位有效） */
        u32 b32 = (u32)(*(u64 *)&v);
        bits = (u64)b32;
        if (((b32 >> 23) & 0xff) == 0xff) {
            if ((b32 & 0x7fffff) != 0) {
                write_all(FD_STDOUT, "NaN", 3);
            } else {
                write_all(FD_STDOUT, (b32 >> 31) ? "-Inf" : "+Inf", (b32 >> 31) ? 4 : 4);
            }
            return;
        }
        u64 frac = b32 & 0x7fffff;
        int ef = (int)((b32 >> 23) & 0xff);
        u64 m;
        int e;
        if (ef == 0) {
            m = frac;
            e = -149;
        } else {
            m = frac | (1ULL << 23);
            e = ef - 150;
        }
        char out[64];
        int n = fmt_bits(m, e, (int)(b32 >> 31), out);
        write_all(FD_STDOUT, out, (u64)n);
        return;
    }
    bits = *(u64 *)&v;
    if (is_nan_f64(bits)) {
        write_all(FD_STDOUT, "NaN", 3);
        return;
    }
    if (is_inf_f64(bits)) {
        if (bits >> 63) {
            write_all(FD_STDOUT, "-Inf", 4);
        } else {
            write_all(FD_STDOUT, "+Inf", 4);
        }
        return;
    }
    u64 frac = bits & ((1ULL << 52) - 1);
    int ef = (int)((bits >> 52) & 0x7ff);
    u64 m;
    int e;
    if (ef == 0) {
        m = frac;
        e = -1074;
    } else {
        m = frac | (1ULL << 52);
        e = ef - 1075;
    }
    char out[64];
    int n = fmt_bits(m, e, (int)(bits >> 63), out);
    write_all(FD_STDOUT, out, (u64)n);
}

/* `println` 复数：`(re+imi)`（`C9` §4.3）。 */
void rt_print_complex(double re, double im) {
    write_all(FD_STDOUT, "(", 1);
    rt_print_f64(re, 0);
    if (*(u64 *)&im >> 63) {
        write_all(FD_STDOUT, "-", 1);
        double a = -im;
        rt_print_f64(a, 0);
    } else {
        write_all(FD_STDOUT, "+", 1);
        rt_print_f64(im, 0);
    }
    write_all(FD_STDOUT, "i)", 2);
}

void rt_print_complex64(float re, float im) {
    write_all(FD_STDOUT, "(", 1);
    double dre = (double)re;
    rt_print_f64(dre, 1);
    if (im < 0.0f || (im == 0.0f && 1.0f / im < 0.0f)) {
        write_all(FD_STDOUT, "-", 1);
        double a = (double)(-im);
        rt_print_f64(a, 1);
    } else {
        write_all(FD_STDOUT, "+", 1);
        rt_print_f64((double)im, 1);
    }
    write_all(FD_STDOUT, "i)", 2);
}
