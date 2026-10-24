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
