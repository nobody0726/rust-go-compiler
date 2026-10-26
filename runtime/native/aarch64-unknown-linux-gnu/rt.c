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
typedef unsigned short u16;
typedef unsigned char u8;

#define SYS_write 64
#define SYS_exit_group 94
#define SYS_munmap 215
#define SYS_mmap 222

/* mmap 的两个常量（`asm-generic/mman-common.h` 的值，不是猜的）。 */
#define PROT_READ_WRITE 3
#define MAP_PRIVATE_ANON 0x22 /* MAP_PRIVATE(2) | MAP_ANONYMOUS(0x20) */

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

/* 六个参数的系统调用（`mmap` 要 6 个）。 */
static i64 sys6(i64 n, i64 a, i64 b, i64 c, i64 d, i64 e, i64 f) {
    register i64 x8 __asm__("x8") = n;
    register i64 x0 __asm__("x0") = a;
    register i64 x1 __asm__("x1") = b;
    register i64 x2 __asm__("x2") = c;
    register i64 x3 __asm__("x3") = d;
    register i64 x4 __asm__("x4") = e;
    register i64 x5 __asm__("x5") = f;
    __asm__ volatile("svc #0"
                     : "+r"(x0)
                     : "r"(x8), "r"(x1), "r"(x2), "r"(x3), "r"(x4), "r"(x5)
                     : "memory");
    return x0;
}

void rt_exit(i64 code) {
    sys3(SYS_exit_group, code, 0, 0);
    for (;;) {
    }
}

/* 短写必须循环（`C5` §2.3 的已知缺口，这里补上）。 */
void *rt_arena_alloc(u64 size, u64 align);
/* `panic_set` / `rt_equal` 的定义在文件后半，但前面（`rt_panic_assert` /
 * `rt_iface_eq`）就要用 —— C99 不允许隐式声明，必须前向。 */
static void panic_set(const char *p, u64 n);

/* 捕获模式（`M9/Phase 5`）：`panic` 的文本要**先攒起来**再存成状态，
 * 而不是直接写出去 —— 因为 `recover()` 可能把它救回来，那就不该有输出。
 *
 * 拦在 `write_all` 这一层而不是给每个 `rt_panic_*` 各写一套格式化：
 * 那 9 个函数的文本格式各不相同（整数 / 字符串 / 浮点 / 复数 / 越界 / 断言），
 * 但它们**都**经过 `write_all`。一处拦截，九处受益。 */
static i64 g_capturing;
static char *g_cap_buf;
static u64 g_cap_len;
static u64 g_cap_cap;

static void write_all(i64 fd, const char *p, u64 n) {
    if (g_capturing) {
        if (g_cap_len + n > g_cap_cap) {
            u64 nc = g_cap_cap == 0 ? 64 : g_cap_cap;
            while (nc < g_cap_len + n) {
                nc *= 2;
            }
            char *nb = rt_arena_alloc(nc, 8);
            for (u64 i = 0; i < g_cap_len; i++) {
                nb[i] = g_cap_buf[i];
            }
            g_cap_buf = nb;
            g_cap_cap = nc;
        }
        for (u64 i = 0; i < n; i++) {
            g_cap_buf[g_cap_len + i] = p[i];
        }
        g_cap_len += n;
        return;
    }
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

/* 逐字节比较（聚合的 `==` / `!=`）。返回 -1 / 0 / 1。 */
i64 rt_memcmp(const void *a, const void *b, u64 n) {
    const u8 *x = (const u8 *)a;
    const u8 *y = (const u8 *)b;
    for (u64 i = 0; i < n; i++) {
        if (x[i] != y[i]) {
            return x[i] < y[i] ? -1 : 1;
        }
    }
    return 0;
}

/* ── 分配：chunk 链表 + bump（`C5` §7.2 / `D-M7-7`；M9/Task 7 改成可增长）── */

/* # 为什么从「一块 8 MiB 的 .bss」改成「mmap 的 chunk 链表」
 *
 * M7 的静态 arena 有两个问题：
 * ① 上限写死 8 MiB —— 复合类型（slice/map/闭包）一上来用量翻倍，
 *    `panic: out of arena` 会变成常态；
 * ② 它**不在** `RGOC_HEAP_MAX` 的口径里，于是「堆上限」这个概念有两半。
 *
 * M9 起只有一个堆：mmap 出来的 chunk 串成链，bump 分配。
 *
 * # 为什么每个 chunk 16 MiB 起
 *
 * 一次 mmap 一个 4 KiB 页会让「大量小对象」变成大量系统调用。16 MiB 是
 * 折中：`turing.go` 这种几千次分配的程序只 mmap 一次，而空程序也只 mmap
 * 一次（惰性：第一次分配才 mmap）。
 */
#define CHUNK_MIN (16UL * 1024UL * 1024UL)
#define HEAP_LIMIT_DEFAULT (256UL * 1024UL * 1024UL)

typedef struct Chunk {
    struct Chunk *next;
    u64 size; /* data 容量（不含本头） */
    u64 used; /* bump 到哪了 */
} Chunk;

static Chunk *heap;
static u64 heap_committed; /* 已 mmap 的字节（含头），上限按它算 */
static u64 heap_limit = HEAP_LIMIT_DEFAULT;

/* 打印 `runtime error: out of memory (heap limit N bytes)` 并退出 2。
 *
 * 文本与 `C9` §6 一致（**不带** `panic: ` 前缀 —— 它不是 recoverable 的
 * panic，而是 fatal，与 `fatal error: all goroutines are asleep` 同类）。 */
static void rt_fatal_oom(u64 limit) {
    const char *a = "runtime error: out of memory (heap limit ";
    u64 n = 0;
    while (a[n] != 0) {
        n++;
    }
    write_all(FD_STDERR, a, n);
    char buf[20];
    u64 v = limit, len = 0;
    if (v == 0) {
        buf[len++] = '0';
    }
    char tmp[20];
    u64 t = 0;
    while (v > 0) {
        tmp[t++] = (char)('0' + (v % 10));
        v /= 10;
    }
    while (t > 0) {
        buf[len++] = tmp[--t];
    }
    write_all(FD_STDERR, buf, len);
    const char *b = " bytes)\n";
    n = 0;
    while (b[n] != 0) {
        n++;
    }
    write_all(FD_STDERR, b, n);
    rt_exit(2);
}

/* 已提交的字节（上限按它算）与上限本身（`RT-MEM-08` 的 ◐：只报数字）。 */
u64 rt_heap_used(void) { return heap_committed; }
u64 rt_heap_limit(void) { return heap_limit; }

/* 读环境变量 `RGOC_HEAP_MAX`（十进制字节）。
 *
 * # 为什么要 `_start` 把 `sp` 交进来
 *
 * freestanding 没有 `getenv`，也没有 libc 帮忙保存 `envp`。而 Linux 的进程
 * 入口栈布局是固定的：`sp → argc`，`argv = sp+8`，`envp = argv + argc + 1`。
 * 所以 `_start` 一开始就把 `sp` 传进来，这里扫一遍。
 *
 * 读不到就用缺省（256 MiB）。**不**因为「读不到」而报错 —— 那会让
 * 「直接跑产物」变成一件需要额外配置的事。 */
static int starts_with(const char *s, const char *p) {
    u64 i = 0;
    while (p[i] != 0) {
        if (s[i] != p[i]) {
            return 0;
        }
        i++;
    }
    return 1;
}

static u64 parse_u64(const char *s) {
    u64 v = 0;
    int any = 0;
    while (*s >= '0' && *s <= '9') {
        v = v * 10 + (u64)(*s - '0');
        s++;
        any = 1;
    }
    return any ? v : 0;
}

void rt_init(u64 sp) {
    u64 argc = *(u64 *)sp;
    char **argv = (char **)(sp + 8);
    char **envp = argv + argc + 1;
    for (u64 i = 0; envp[i] != 0; i++) {
        const char *e = envp[i];
        if (starts_with(e, "RGOC_HEAP_MAX=")) {
            u64 v = parse_u64(e + 14); /* strlen("RGOC_HEAP_MAX=") == 14 */
            /* 小于一页的上限没有意义（连一个 chunk 都放不下），当作没设。 */
            if (v >= 4096) {
                heap_limit = v;
            }
        }
    }
}

/* mmap 一个新 chunk 挂到链头。`need` 是「至少要能装下的 data 字节」。 */
void rt_grow(u64 need) {
    u64 cap = CHUNK_MIN;
    if (need > cap) {
        cap = need;
    }
    /* 向上取整到 4 KiB（mmap 会自己取整，但 `heap_committed` 要对得上）。 */
    cap = (cap + 4095UL) & ~4095UL;
    u64 total = cap + sizeof(Chunk);
    if (heap_committed + total > heap_limit) {
        rt_fatal_oom(heap_limit);
    }
    i64 r = sys6(SYS_mmap, 0, (i64)total, PROT_READ_WRITE, MAP_PRIVATE_ANON, -1, 0);
    if (r < 0) {
        /* mmap 失败（ENOMEM 等）：按上限超限处理 —— 对调用方是同一件事：
         * 「要不到内存」。文本带上限，因为那是用户能调的那个数。 */
        rt_fatal_oom(heap_limit);
    }
    Chunk *c = (Chunk *)r;
    c->next = heap;
    c->size = cap;
    c->used = 0;
    heap = c;
    heap_committed += total;
}

/* ── 超大帧的存储（`M9/Phase 5c`，`R-9-22`）──
 *
 * # 为什么帧需要「不在栈上」这一档
 *
 * 帧是**定长**的（`C5` §7）：序言一条 `sub sp, sp, #frame_size` 就把它整块要下来。
 * 只要帧比栈上限大（Linux 缺省 `ulimit -s` 是 8 MiB），第一条落在帧底部的
 * 访存就 SIGSEGV —— 而 Go 那边栈是**动态增长**的，同样的程序只是慢一点。
 *
 * 实测语料 `nilptr2.go`：`use(*bigstructp)` 里 `BigStruct` 有 `[128<<20]byte`，
 * 逐字拷进本帧后帧是 128 MiB。它不是「实现得不够快」，是**这台机器的栈装不下**。
 *
 * 所以超过 `STACK_FRAME_LIMIT` 的帧改成从 mmap 拿一块零页当帧底，`fp` 指到
 * 「块尾 - 16」（保存区那 16 字节在**栈上**，不在这块里）。槽偏移的算法一个字
 * 都没变 —— 它们本来就是「相对 `fp` 的负数」。
 *
 * 用 mmap 而不是 `rt_arena_alloc`：arena 是**不回收**的堆，循环里调一个超大帧
 * 函数会一路吃光 `RGOC_HEAP_MAX`；这里成对 mmap/munmap，调用结束就还。
 */
void *rt_stack_frame_alloc(u64 n) {
    u64 pages = (n + 4095UL) & ~4095UL;
    i64 r = sys6(SYS_mmap, 0, (i64)pages, PROT_READ_WRITE, MAP_PRIVATE_ANON, -1, 0);
    if (r < 0) {
        rt_fatal_oom(pages);
    }
    /* mmap 给的是零页 ⇒ 帧的零值语义（`C5` §7.2）天然满足，不必再 memset */
    return (void *)r;
}

void rt_stack_frame_free(void *p, u64 n) {
    u64 pages = (n + 4095UL) & ~4095UL;
    sys3(SYS_munmap, (i64)p, (i64)pages, 0);
}

/* 零大小对象的固定地址（`zerosize.go` 要求 `new([0]int)` 两次相等）。
 *
 * ⚠️ 这个符号**不在这里定义**：它由生成的 `.s` 定义（`module.rs` ①），
 * 因为零大小**全局**也要落在同一地址上（`.set g<id>, rt_zerobase`）。
 * 「同一个地址」只能有一处来源 —— 两边各自定义就变成两个地址了。 */
extern u8 rt_zerobase[];

void *rt_arena_alloc(u64 size, u64 align) {
    if (size == 0) {
        return rt_zerobase;
    }
    if (align == 0) {
        align = 8;
    }
    for (;;) {
        Chunk *c = heap;
        if (c != 0) {
            u64 p = (c->used + align - 1) & ~(align - 1);
            if (p + size <= c->size) {
                c->used = p + size;
                void *r = (void *)((u8 *)(c + 1) + p);
                /* 保留 M7 的语义：**这块内存一定是零**。
                 * mmap 给的就是零页，所以这条 memset 目前是冗余的 ——
                 * 但它是「arena 的内存是零」这条契约的**唯一**执行点，
                 * 而 M10 一旦开始复用对象，冗余就消失了。 */
                rt_memset(r, 0, size);
                return r;
            }
        }
        rt_grow(size + align);
    }
}

/* ── 类型元数据与统一分配入口（M9/Task 6）── */

/* `TypeDesc` 的**内存**布局（`C5` §10 冻结的 40 字节）。
 *
 * 这里重抄一份而不是包含一个头文件：runtime 是 freestanding 的，没有任何构建
 * 系统的头文件搜索；而 `_Static_assert` 把「两份定义是否一致」变成**编译期**
 * 判据 —— Rust 侧 `T-META-01` 逐字段 `offset_of`，C 侧这里逐字段 `offsetof`。
 *
 * `flags` 的三位（与 `rgoc_meta::desc::flags` 同值）：
 *   bit0 COMPARABLE / bit1 HAS_POINTERS / bit2 NEEDS_ZERO
 */
typedef struct {
    u32 size;
    u32 ptrdata;
    u32 hash;
    u8 kind;
    u8 flags;
    u16 pad;
    const u8 *gcdata;
    const void *elem;
    const void *methods;
} TypeDesc;

_Static_assert(sizeof(TypeDesc) == 40, "TypeDesc 必须是 40 字节（C5 §10）");

/* 值相等（定义在文件后半；`rt_iface_eq` 在它之前就要用）。 */
int rt_equal(const TypeDesc *d, const void *a, const void *b);
_Static_assert(__builtin_offsetof(TypeDesc, gcdata) == 16, "gcdata 在 +16");
_Static_assert(__builtin_offsetof(TypeDesc, elem) == 24, "elem 在 +24");
_Static_assert(__builtin_offsetof(TypeDesc, methods) == 32, "methods 在 +32");

#define DESC_FLAG_NEEDS_ZERO 4

/* 对齐不达标时的计数（探针；正常永远是 0）。
 *
 * 为什么要一个计数器而不只是断言：断言只在**发生的那一刻**能报，而这条
 * 探针让「历史上有过几次」变成可读的状态。M9 的分配器保证 8 字节对齐，
 * 所以它是 0 —— 而它是 0 这件事本身，是 `T-MEM-05` 的判据。 */
static u64 rt_align_bad;

u64 rt_probe_align(void) { return rt_align_bad; }

/* 统一分配入口：`[+0 desc][+8 data...]`（`M9-design.md` §3.2）。
 *
 * `need_zero` 由**编译期**按 `needs_zero(类型)` 传入（含指针或含浮点 ⇒ 1）。
 * 现在它看起来多余（arena 拿到的都是新页，本来就是零），但 M10 的 GC 一旦
 * 开始复用对象，它就变成「不清零就会把上一条命的指针交给 GC」的唯一防线。
 * 所以现在**不**省掉这个参数 —— 省掉之后加回来要改所有调用点。
 */
void *rt_alloc(const TypeDesc *desc, i64 need_zero) {
    u64 size = desc->size;
    if (size == 0) {
        /* 零大小对象：所有实例共享 `rt_zerobase`（`zerosize.go` 要求
         * `&x == new([0]int)`）。它没有可写的 data，也不写 desc ——
         * 那块内存是**所有**零大小类型共用的，写谁的 desc 都是错的。 */
        return rt_zerobase;
    }
    u8 *p = (u8 *)rt_arena_alloc(size + 8, 8);
    if (((u64)p & 7UL) != 0) {
        rt_align_bad++;
        const char *m = "panic: misaligned allocation\n";
        u64 n = 0;
        while (m[n] != 0) {
            n++;
        }
        write_all(FD_STDERR, m, n);
        rt_exit(2);
    }
    *(const TypeDesc **)p = desc;
    if (need_zero != 0) {
        rt_memset(p + 8, 0, size);
    }
    /* 返回 **data**（`p + 8`），不是块首：Go 侧的指针必须指到值上，
     * 否则字段偏移 0 会与对象头重叠（`M9` 的 `D-M9-1` 从没真正生效）。 */
    return p + 8;
}

/* ── 接口：装箱、itab 与断言（M9/Task 29–33）── */

/* `MethodTable` / `MethodEntry` 的内存布局（`C5` §10 冻结）。
 * 与 `rgoc_meta::desc` 的 `#[repr(C)]` 逐字段对应 —— 两边各有 `offset_of`
 * 判据（Rust 侧 `方法表的字节形状`，C 侧下面两条 `_Static_assert`）。 */
typedef struct {
    u32 name_hash;
    u32 sig_hash;
    const void *fn;
} MethodEntry;

typedef struct {
    u32 count;
    u32 pad;
} MethodTable;

_Static_assert(sizeof(MethodEntry) == 16, "MethodEntry 必须是 16 字节（C5 §10）");
_Static_assert(sizeof(MethodTable) == 8, "MethodTable 表头必须是 8 字节");

/* 静态 ITab：`{ iface, concrete, fn[] }`，`fn` 从 **+16** 起。
 * `isel` 的 `tab.fn[slot]` 就是 `[tab + 16 + 8*slot]`。 */
typedef struct {
    const TypeDesc *iface;
    const TypeDesc *concrete;
} ITab;

/* 装箱：分配一个 `T` 并把值搬进去。
 *
 * 零大小类型不分配（所有实例共享 `rt_zerobase`）—— `rt_alloc` 已经这么做了，
 * 这里只是别去 memcpy 0 字节之外的东西。
 *
 * **指针动态类型不走这里**：`isel` 直接把指针值当 `data`（官方同口径）。
 * 理由不是省一次分配，而是「两个 nil 指针的接口必须相等」——
 * 装箱会让它们指向两个不同的盒子。 */
void *rt_box(const TypeDesc *d, const void *src) {
    void *p = rt_alloc(d, 0);
    if (d->size != 0) {
        rt_memcpy(p, src, d->size);
    }
    return p;
}

/* 运行期构造 itab（`D-M9-9` 的兜底路径）：`x.(J)` 的 `(J, T)` 对在编译期
 * 枚举不出来（`T` 是 `x` 的静态类型的所有可能实现）。
 *
 * **不缓存**（`D-M9-16`）：没有并发、语料里最多几百次，缓存的收益抵不上
 * 一张 `inter×concrete` 哈希表 + 它的正确性负担。
 *
 * 返回 0 表示 `concrete` 没实现 `iface`（缺方法 / 签名不同）。
 * 内存用 `rt_arena_alloc`（**不带对象头**）：itab 是元数据，不是 Go 对象 ——
 * 与切片底层数组同一条理由（`R-9-5`）。 */
void *rt_itab(const TypeDesc *iface, const TypeDesc *concrete) {
    const MethodTable *imt = (const MethodTable *)iface->methods;
    const MethodTable *cmt = (const MethodTable *)concrete->methods;
    if (imt == 0 || imt->count == 0) {
        return 0;
    }
    if (cmt == 0) {
        return 0;
    }
    u64 n = (u64)imt->count;
    u8 *tab = (u8 *)rt_arena_alloc((2 + n) * 8, 8);
    ((const TypeDesc **)tab)[0] = iface;
    ((const TypeDesc **)tab)[1] = concrete;
    const MethodEntry *ie = (const MethodEntry *)((const u8 *)imt + 8);
    const MethodEntry *ce = (const MethodEntry *)((const u8 *)cmt + 8);
    for (u64 i = 0; i < n; i++) {
        const void *fn = 0;
        for (u32 j = 0; j < cmt->count; j++) {
            if (ce[j].name_hash == ie[i].name_hash && ce[j].sig_hash == ie[i].sig_hash) {
                fn = ce[j].fn;
                break;
            }
        }
        if (fn == 0) {
            return 0;
        }
        ((const void **)tab)[2 + i] = fn;
    }
    return tab;
}

/* 接口相等（`M9/Task 35`）。
 *
 * # 为什么不能按字节比那两个字
 *
 * `data` 指向**盒子**（非指针动态类型），两个 `any("abc")` 各有各的盒子 ⇒
 * 按位比永远不等。Go 的语义是「动态类型相同 **且** 动态值相等」，所以要
 * 先比第 0 个字（`tab` / `type`），再按**动态类型**比 `data`：
 *
 * | 动态类型 | `data` 是什么 | 怎么比 |
 * |---|---|---|
 * | `*T`（`Kind::Pointer`）| 指针**值**本身 | 比两个字 |
 * | 其余 | 指向值的盒子 | `memcmp(desc->size)` |
 *
 * `eface` 参数：`any` / `interface{}` 的第 0 个字**就是** desc，有方法的接口
 * 第 0 个字是 itab（`[+8]` 才是 desc）—— 运行期分不出来，只能由调用点告诉。
 *
 * 实测：`ken/interbasic.go` 的 `panic("a")`（两个 `any("abc")` 判不等）。 */
int rt_iface_eq(u64 a0, u64 a1, u64 b0, u64 b1, i64 eface) {
    if (a0 != b0) {
        return 0;
    }
    if (a0 == 0) {
        /* 两边都是 nil 接口 */
        return 1;
    }
    const TypeDesc *d = eface ? (const TypeDesc *)a0 : *(const TypeDesc **)(a0 + 8);
    if (d->kind == 7 /* Kind::Pointer */) {
        /* 指针动态类型：`data` 是**值本身**，比两个字 */
        return a1 == b1;
    }
    /* 其余：`data` 指向盒子 ⇒ 用**同一套**值相等（`rt_equal`，字符串按内容）。
     * ⚠️ 这里不能用 `memcmp`：`any("abc")` 的盒子里是 `{ptr,len}`，按字节比
     * 就是在比**指针** —— 两个内容相同的字面量各有各的 `.rodata` 时判不等
     * （实测 `ken/interbasic.go` 的 `panic("a")`）。 */
    return rt_equal(d, (const void *)a1, (const void *)b1);
}

/* 断言失败（`v := x.(T)` 的单值形态）。
 *
 * ⚠️ 文本与**解释器**不一致：官方与解释器都写
 * `interface conversion: <I> is <T>, not <U>`，而那要**类型名**——
 * 名字不在 `TypeDesc` 里（`U-M9-04`，归 M12）。这里只给前缀 + `failed`，
 * 记在 `G-M9-03`。D 腿的语料不该走到这里（走到就是真 bug）。 */
void rt_panic_assert(void) {
    const char *m = "interface conversion: failed";
    u64 n = 0;
    while (m[n] != 0) {
        n++;
    }
    panic_set(m, n);
}

/* 前向声明：`rt_map_set` 要用它，而它的定义在 panic 那一节（文件后半）。 */
void rt_panic_nilmap(void);

/* ── 映射（M9/Task 18–23；`D-M9-8` 的开放寻址）── */

/* 槽状态。`TOMB` 与 `EMPTY` 必须分开：`TOMB` **不终止探测** ——
 * 否则「删过的键」会把后面的键挡在探测链外（查不到）。 */
#define MAP_EMPTY 0
#define MAP_LIVE 1
#define MAP_TOMB 2

/* `MapObj` 的形状（`M9-design.md` §3.7）。单字指针指向它。 */
typedef struct {
    u64 count;          /* 活条目数 */
    u64 cap;            /* 槽数，恒为 2 的幂 */
    u32 key_size;
    u32 val_size;
    const TypeDesc *kdesc;
    const TypeDesc *vdesc;
    u8 *slots;          /* cap × stride 字节 */
} MapObj;

static u64 align8(u64 n) { return (n + 7UL) & ~7UL; }

/* 一个槽的字节数：状态 1 字节（占 8）+ key + val，各自按 8 对齐。 */
static u64 slot_stride(const MapObj *m) {
    return 8UL + align8((u64)m->key_size) + align8((u64)m->val_size);
}

static u8 *slot_at(const MapObj *m, u64 i) { return m->slots + i * slot_stride(m); }

static u8 *slot_key(const MapObj *m, u8 *slot) { return slot + 8; }

static u8 *slot_val(const MapObj *m, u8 *slot) { return slot + 8 + align8((u64)m->key_size); }

/* 类型 hash（FNV-1a 32 位）。
 *
 * # 按 kind 分派的三个分支
 *
 * * `String`：按**内容**（`ptr` 指向的 `len` 字节）—— 两个内容相同的字符串
 *   是两个不同的对象，哈希必须相同。
 * * 其余标量 / 指针 / 单字：按那 8 字节。
 * * 聚合（Array / Struct）：按 `size` 字节的**原始字节**。
 *
 * # ⚠️ 聚合的这条是**有记录的窄化**（`U-M9-11`）
 *
 * 规范要求聚合按**字段内容**比较。原始字节对「只含标量的聚合」与内容比较等价，
 * 但对「含指针的聚合」（如 `struct{ s string }`）比较的是**指针**而不是内容 ——
 * 两个内容相同的 `struct{ s string }` 会被判成不同的键。
 *
 * 修法需要「字段 desc 表」（每个字段的类型 + 偏移），那是 M12 反射元数据的一部分；
 * M9 的 typedesc 刻意只有 8 个字段（`D-M9-2`），塞不进去。已记 `G-M9-03`。
 */
u32 rt_hash(const TypeDesc *d, const void *p) {
    u32 h = 2166136261u;
    u64 n = d->size;
    const u8 *b = (const u8 *)p;
    if (d->kind == 6 /* Kind::String */) {
        /* 两个字：ptr + len。按内容哈希。 */
        const u8 *sp = *(const u8 *const *)p;
        n = *(const u64 *)((const u8 *)p + 8);
        b = sp;
    }
    for (u64 i = 0; i < n; i++) {
        h ^= (u32)b[i];
        h *= 16777619u;
    }
    return h;
}

/* 键相等（与 `rt_hash` 同一套 kind 分派，见上面的窄化说明）。 */
int rt_equal(const TypeDesc *d, const void *a, const void *b) {
    if (d->kind == 6 /* Kind::String */) {
        u64 na = *(const u64 *)((const u8 *)a + 8);
        u64 nb = *(const u64 *)((const u8 *)b + 8);
        if (na != nb) {
            return 0;
        }
        const u8 *pa = *(const u8 *const *)a;
        const u8 *pb = *(const u8 *const *)b;
        for (u64 i = 0; i < na; i++) {
            if (pa[i] != pb[i]) {
                return 0;
            }
        }
        return 1;
    }
    u64 n = d->size;
    if (n == 0) {
        /* 零大小类型（`struct{}`）：所有值都相等 —— 与规范一致。 */
        return 1;
    }
    const u8 *pa = (const u8 *)a;
    const u8 *pb = (const u8 *)b;
    for (u64 i = 0; i < n; i++) {
        if (pa[i] != pb[i]) {
            return 0;
        }
    }
    return 1;
}

/* 分配 `cap` 个槽并清零。 */
static void map_alloc_slots(MapObj *m, u64 cap) {
    m->cap = cap;
    u64 bytes = cap * slot_stride(m);
    m->slots = (u8 *)rt_arena_alloc(bytes, 8);
    rt_memset(m->slots, 0, bytes);
}

/* `make(map[K]V[, n])` —— `n` 只是容量提示，M9 从 8 个槽起步（靠 rehash 长）。
 *
 * # 为什么忽略 `n`
 *
 * 它只影响性能，不影响可观测语义（`len` 只数活条目，`cap` 对 map 不可见）。
 * 按 `n` 预分配反而会让「小 map 大 n」浪费一堆 arena —— 而 M9 没有 GC。
 */
/* 真正的构造函数。 */
static MapObj *map_new(const TypeDesc *kdesc, const TypeDesc *vdesc) {
    /* 头用 `rt_arena_alloc` 而不是 `rt_alloc`：`MapObj` 不是 Go 值
     * （程序里看到的只是一个 `*MapObj` 指针），给它写对象头会让 M10 把它当
     * 一个「kind=Struct 的对象」去扫 —— 而它的字段是宿主结构。 */
    MapObj *m = (MapObj *)rt_arena_alloc(sizeof(MapObj), 8);
    m->count = 0;
    m->key_size = kdesc->size;
    m->val_size = vdesc->size;
    m->kdesc = kdesc;
    m->vdesc = vdesc;
    map_alloc_slots(m, 8);
    return m;
}

/* `make(map[K]V)`。 */
void *rt_map_make(const TypeDesc *kdesc, const TypeDesc *vdesc) {
    return map_new(kdesc, vdesc);
}

u64 rt_map_len(const void *mp) {
    const MapObj *m = (const MapObj *)mp;
    return m == 0 ? 0 : m->count;
}

/* 在 `m` 里找 `k` 的槽。返回槽指针；`*found` 说明是命中还是遇到空槽。
 *
 * # 三条不变式
 *
 * 1. **`TOMB` 不终止探测**（见文件头的说明）。
 * 2. 记下**第一个** `TOMB`（`first_tomb`）：插入时优先复用它，否则删得越多
 *    表越脏、探测链越长。
 * 3. `cap` 是 2 的幂 ⇒ `& (cap-1)` 代替取模。
 */
static u8 *map_probe(MapObj *m, const void *k, int *found, u8 **first_tomb) {
    u64 mask = m->cap - 1;
    u64 i = (u64)rt_hash(m->kdesc, k) & mask;
    u8 *tomb = 0;
    for (u64 n = 0; n < m->cap; n++) {
        u8 *slot = slot_at(m, i);
        u8 st = slot[0];
        if (st == MAP_LIVE) {
            if (rt_equal(m->kdesc, slot_key(m, slot), k)) {
                *found = 1;
                if (first_tomb != 0) {
                    *first_tomb = tomb;
                }
                return slot;
            }
        } else if (st == MAP_EMPTY) {
            *found = 0;
            if (first_tomb != 0) {
                *first_tomb = tomb;
            }
            return slot;
        } else if (tomb == 0) {
            tomb = slot;
        }
        i = (i + 1) & mask;
    }
    /* 全表都是 LIVE/TOMB 且没有空槽：插入必然失败 —— 靠 rehash 兜住
     * （负载因子 0.75 ⇒ 活条目最多 3/4，所以总会有空槽或 TOMB 可复用）。 */
    *found = 0;
    if (first_tomb != 0) {
        *first_tomb = tomb;
    }
    return tomb;
}

/* 扩容：cap 翻倍 + 全量重插（**不做渐进搬迁**，`U-M9-08`）。 */
static void map_grow(MapObj *m) {
    u8 *old_slots = m->slots;
    u64 old_cap = m->cap;
    u64 old_stride = slot_stride(m);
    map_alloc_slots(m, old_cap * 2);
    for (u64 i = 0; i < old_cap; i++) {
        u8 *slot = old_slots + i * old_stride;
        if (slot[0] != MAP_LIVE) {
            continue;
        }
        int found = 0;
        u8 *dst = map_probe(m, slot_key(m, slot), &found, 0);
        dst[0] = MAP_LIVE;
        rt_memcpy(slot_key(m, dst), slot_key(m, slot), m->key_size);
        rt_memcpy(slot_val(m, dst), slot + 8 + align8((u64)m->key_size), m->val_size);
    }
}

/* `m[k]`（单值）：写零值到 `out`，返回 1 表示命中。 */
int rt_map_get(const void *mp, const void *k, void *out, u64 vsize) {
    MapObj *m = (MapObj *)mp;
    if (m == 0) {
        /* nil map 读：写**值的零值**。`vsize` 由 isel 给 ——
         * 没有 map 头就拿不到 `val_size`（这正是 nil 的含义）。 */
        rt_memset(out, 0, vsize);
        return 0;
    }
    int found = 0;
    u8 *slot = map_probe(m, k, &found, 0);
    if (!found) {
        rt_memset(out, 0, m->val_size);
        return 0;
    }
    rt_memcpy(out, slot_val(m, slot), m->val_size);
    return 1;
}

/* `m[k] = v`。nil map ⇒ panic（规范如此）。 */
void rt_map_set(void *mp, const void *k, const void *v) {
    MapObj *m = (MapObj *)mp;
    if (m == 0) {
        rt_panic_nilmap();
        return;
    }
    /* 负载因子 0.75：`count*4 >= cap*3` 就长。先长再插 ——
     * 反过来会在「刚满的表」上探测一整圈。 */
    if ((m->count + 1) * 4 >= m->cap * 3) {
        map_grow(m);
    }
    int found = 0;
    u8 *tomb = 0;
    u8 *slot = map_probe(m, k, &found, &tomb);
    if (found) {
        rt_memcpy(slot_val(m, slot), v, m->val_size);
        return;
    }
    if (slot[0] != MAP_LIVE && tomb != 0) {
        slot = tomb;
    }
    slot[0] = MAP_LIVE;
    rt_memcpy(slot_key(m, slot), k, m->key_size);
    rt_memcpy(slot_val(m, slot), v, m->val_size);
    m->count++;
}

/* `delete(m, k)`：不存在的键是**静默无操作**（规范如此）。 */
void rt_map_del(void *mp, const void *k) {
    MapObj *m = (MapObj *)mp;
    if (m == 0) {
        return;
    }
    int found = 0;
    u8 *slot = map_probe(m, k, &found, 0);
    if (!found) {
        return;
    }
    slot[0] = MAP_TOMB;
    m->count--;
}

/* `range m` 的一步：从 `idx` 起找下一个 `LIVE` 槽，把键/值写进 `kout`/`vout`，
 * 返回**下一个起点**（> 0）；返回 0 表示迭代结束。
 *
 * # 为什么用「下标 + 返回值」而不是迭代器对象
 *
 * 迭代器对象要多一次分配、还要在 SSA 里维护它的生命周期；而「下一站下标」这个
 * 返回值天然是循环变量。`idx` 从 0 开始、返回 `i+1`，所以 0 可以当「结束」。
 *
 * # 迭代中删除
 *
 * 按 `state` 判 ⇒ 已删的（`TOMB`）不再产出 —— 规范要求「迭代开始后被删的条目
 * 不产出」，这条天然满足。**迭代中新增**可能产出也可能不（规范允许两者）。
 */
u64 rt_map_iter(const void *mp, u64 idx, void *kout, void *vout) {
    const MapObj *m = (const MapObj *)mp;
    if (m == 0) {
        return 0;
    }
    for (u64 i = idx; i < m->cap; i++) {
        u8 *slot = slot_at(m, i);
        if (slot[0] != MAP_LIVE) {
            continue;
        }
        rt_memcpy(kout, slot_key(m, slot), m->key_size);
        rt_memcpy(vout, slot_val(m, slot), m->val_size);
        return i + 1;
    }
    return 0;
}

/* ── 切片（M9/Task 11；`C9` 的 `{ptr,len,cap}` 三字）── */

/* 与 `isel` 的槽布局**逐字对齐**：字 0 = ptr、字 1 = len、字 2 = cap。
 * 三处（这里、`isel` 的 `slice` 系列、`C5` §10）必须同形。 */
typedef struct {
    void *ptr;
    u64 len;
    u64 cap;
} Slice;

/* `make([]T, len[, cap])`。
 *
 * # 三条约定
 *
 * 1. **`len == 0 && cap == 0` ⇒ `ptr = NULL`**（Go 的 nil slice 与空 slice
 *    在 `== nil` 上不同，但都 `len == 0`）。这里给的是 `cap == 0` 的那一支：
 *    不分配。
 * 2. **零大小元素**（`[]struct{}`）：不分配，`ptr = rt_zerobase`，`len/cap` 照记。
 *    Go 也这么做（`zerobase`），而且这样 `s[0]` 取地址不会崩。
 * 3. **底层数组没有对象头**（`rt_arena_alloc` 而不是 `rt_alloc`）：它的形状是
 *    `cap * elem->size` 字节的**元素数组**，不是一个 `T`。M9 不记账；M10 的
 *    扫描器要靠「元素 desc + 元素个数」扫它（Go 用 span 的 `elemtype`），
 *    那条契约在 M10 定 —— 记在 `G-M9-01`。
 */
void rt_makeslice(Slice *out, const TypeDesc *elem, u64 len, u64 cap) {
    if (cap < len) {
        /* Go 在这里 panic `makeslice: cap out of range`；M9 先按 len 处理 ——
         * 检查器已拦常量情形，运行期情形归 Phase 5 的 panic 机制。 */
        cap = len;
    }
    out->len = len;
    out->cap = cap;
    if (cap == 0) {
        out->ptr = 0;
        return;
    }
    if (elem->size == 0) {
        out->ptr = rt_zerobase;
        return;
    }
    u64 bytes = cap * (u64)elem->size;
    /* 对齐取元素的对齐（`[]int16` 的底层数组必须是 2 对齐的）——
     * 这里用 8 是保守的：M9 的元素最大对齐就是 8。 */
    out->ptr = rt_arena_alloc(bytes, 8);
}

/* 扩容：cap 够就原地，不够就新分配 + 拷。
 *
 * # 增长策略
 *
 * `max(2*cap, newlen)`。Go 的实际策略更复杂（小于 256 时 2 倍、之后 1.25 倍），
 * 但**cap 的具体值不是可观测语义** —— 程序只能看到「cap >= len」。所以这里
 * 用最简单的 2 倍；`T-SLICE-05` 断言的是「扩容后 `u` 不受影响」而不是某个数。
 *
 * ⚠️ 但 `cap` 在**测试里**是可观测的（`println(cap(t))`）：所以 fixture 的期望值
 * 必须按 2 倍算 —— `t := make([]int,2,2); t = append(t,9)` ⇒ cap 4。
 */
static Slice slice_grow(void *ptr, u64 len, u64 cap, const TypeDesc *elem, u64 extra) {
    Slice out;
    u64 newlen = len + extra;
    if (newlen <= cap) {
        out.ptr = ptr;
        out.len = newlen;
        out.cap = cap;
        return out;
    }
    u64 newcap = cap * 2;
    if (newcap < newlen) {
        newcap = newlen;
    }
    u64 esz = (u64)elem->size;
    void *np;
    if (esz == 0) {
        /* 零大小元素：没有底层数组可言（`[]struct{}` 的 append 永远不分配）。 */
        np = rt_zerobase;
    } else {
        np = rt_arena_alloc(newcap * esz, 8);
        if (ptr != 0 && len != 0) {
            rt_memcpy(np, ptr, len * esz);
        }
    }
    out.ptr = np;
    out.len = newlen;
    out.cap = newcap;
    return out;
}

/* `append(s, v)`：`val` 是那个元素的**地址**（多字元素按 `esz` 整块拷）。 */
void rt_append_one(Slice *out, void *ptr, u64 len, u64 cap, const TypeDesc *elem,
                   const void *val) {
    Slice s = slice_grow(ptr, len, cap, elem, 1);
    u64 esz = (u64)elem->size;
    if (esz != 0) {
        rt_memcpy((u8 *)s.ptr + len * esz, val, esz);
    }
    *out = s;
}

/* `append(s, xs...)`：把 `extra` 个元素从 `src` 拷过来。 */
void rt_append_dots(Slice *out, void *ptr, u64 len, u64 cap, const TypeDesc *elem,
                    u64 extra, const void *src) {
    Slice s = slice_grow(ptr, len, cap, elem, extra);
    u64 esz = (u64)elem->size;
    if (esz != 0 && extra != 0) {
        rt_memcpy((u8 *)s.ptr + len * esz, src, extra * esz);
    }
    *out = s;
}

/* ── 字符串（`C9` §4.3：`string` = `(ptr, len)`，比较按字节）── */

/* `(ptr, len)` 的 16 字节聚合。AAPCS64 下**按两个 `x` 寄存器**传参与返回
 * （≤16 字节、非 HFA 的复合类型）—— 正好对上 `isel` 的 `x0/x1`、`x2/x3` 约定。 */
typedef struct {
    const char *p;
    u64 n;
} rstr;

/* `string([]byte)` / `string([]rune)`（M9/Task 13；`RT-CMP-02` 的边界）。
 *
 * 两者都是**拷贝**（Go 规定转换出来的字符串不与原切片共享存储）：
 * `s := string(b); b[0] = 'X'` 之后 `s` 不变。
 */
rstr rt_bytes2str(const u8 *p, u64 n) {
    rstr out;
    if (n == 0) {
        out.p = "";
        out.n = 0;
        return out;
    }
    char *q = (char *)rt_arena_alloc(n, 1);
    rt_memcpy(q, p, n);
    out.p = q;
    out.n = n;
    return out;
}

/* 单个码点按 UTF-8 编码进 `buf`，返回字节数（与 `rt_i64_to_str` 同一套规则：
 * 越界 / 代理区 ⇒ U+FFFD）。 */
static u64 utf8_put(u8 *buf, u32 r) {
    if (r > 0x10FFFF || (r >= 0xD800 && r <= 0xDFFF)) {
        buf[0] = 0xEF;
        buf[1] = 0xBF;
        buf[2] = 0xBD;
        return 3;
    }
    if (r < 0x80) {
        buf[0] = (u8)r;
        return 1;
    }
    if (r < 0x800) {
        buf[0] = (u8)(0xC0 | (r >> 6));
        buf[1] = (u8)(0x80 | (r & 0x3F));
        return 2;
    }
    if (r < 0x10000) {
        buf[0] = (u8)(0xE0 | (r >> 12));
        buf[1] = (u8)(0x80 | ((r >> 6) & 0x3F));
        buf[2] = (u8)(0x80 | (r & 0x3F));
        return 3;
    }
    buf[0] = (u8)(0xF0 | (r >> 18));
    buf[1] = (u8)(0x80 | ((r >> 12) & 0x3F));
    buf[2] = (u8)(0x80 | ((r >> 6) & 0x3F));
    buf[3] = (u8)(0x80 | (r & 0x3F));
    return 4;
}

rstr rt_runes2utf8(const u32 *p, u64 n) {
    rstr out;
    if (n == 0) {
        out.p = "";
        out.n = 0;
        return out;
    }
    u8 *buf = (u8 *)rt_arena_alloc(n * 4, 1);
    u64 k = 0;
    for (u64 i = 0; i < n; i++) {
        k += utf8_put(buf + k, p[i]);
    }
    out.p = (const char *)buf;
    out.n = k;
    return out;
}


/* `a + b`：结果落在 arena 里（M7 没有 GC，只 bump）。 */
rstr rt_strcat(rstr a, rstr b) {
    u64 n = a.n + b.n;
    /* 空结果也要有**可用的地址**（Go 允许 `len(s)==0` 时取 `&s[0]`）。 */
    char *p = (char *)rt_arena_alloc(n == 0 ? 1 : n, 1);
    for (u64 i = 0; i < a.n; i++) {
        p[i] = a.p[i];
    }
    for (u64 i = 0; i < b.n; i++) {
        p[a.n + i] = b.p[i];
    }
    rstr r;
    r.p = p;
    r.n = n;
    return r;
}

/* 字典序：返回 -1 / 0 / 1。
 *
 * 按**无符号字节**比较（Go 的字符串就是字节序列，不是 Unicode 码点）。
 * 用 `signed char` 会让 `0xFF` 排到 `0x01` 前面 —— 只在非 ASCII 上出错，
 * 正是「看起来对」的那类错。 */
i64 rt_strcmp(rstr a, rstr b) {
    u64 m = a.n < b.n ? a.n : b.n;
    for (u64 i = 0; i < m; i++) {
        u8 x = (u8)a.p[i];
        u8 y = (u8)b.p[i];
        if (x != y) {
            return x < y ? -1 : 1;
        }
    }
    if (a.n == b.n) {
        return 0;
    }
    return a.n < b.n ? -1 : 1;
}

/* `string(i)`：把整数当**码点**编码成 UTF-8（Go 的 `string(rune)` 语义）。
 *
 * 三条容易漏的规则：
 * - 负数、`> 0x10FFFF`、以及代理区 `0xD800..0xDFFF` 一律给 `U+FFFD`（不是截断）；
 * - `U+FFFD` 是 3 字节 `EF BF BD`；
 * - 结果落在 arena 里，**即使 1 字节**也要有稳定地址（`&s[0]` 合法）。 */
rstr rt_i64_to_str(i64 v) {
    char *p = (char *)rt_arena_alloc(4, 1);
    u64 n;
    if (v < 0 || v > 0x10FFFF || (v >= 0xD800 && v <= 0xDFFF)) {
        p[0] = (char)0xEF;
        p[1] = (char)0xBF;
        p[2] = (char)0xBD;
        n = 3;
    } else if (v < 0x80) {
        p[0] = (char)v;
        n = 1;
    } else if (v < 0x800) {
        p[0] = (char)(0xC0 | (v >> 6));
        p[1] = (char)(0x80 | (v & 0x3F));
        n = 2;
    } else if (v < 0x10000) {
        p[0] = (char)(0xE0 | (v >> 12));
        p[1] = (char)(0x80 | ((v >> 6) & 0x3F));
        p[2] = (char)(0x80 | (v & 0x3F));
        n = 3;
    } else {
        p[0] = (char)(0xF0 | (v >> 18));
        p[1] = (char)(0x80 | ((v >> 12) & 0x3F));
        p[2] = (char)(0x80 | ((v >> 6) & 0x3F));
        p[3] = (char)(0x80 | (v & 0x3F));
        n = 4;
    }
    rstr r;
    r.p = p;
    r.n = n;
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

/* ── 指针 / 接口 / 切片 / map 的打印（`M9/Phase 5c`）──
 *
 * 格式表来自 `C9` §5（与 `rgoc-interp::print` 同一张表，`R-9-23`）：
 *
 * | 值 | 文本 |
 * |---|---|
 * | `nil` 指针 / `nil` map | `0x0` |
 * | `nil` 接口 | `(0x0,0x0)` |
 * | `nil` 切片 | `[0/0]0x0` |
 * | 非 nil | 真实地址（官方也不可复现，只求「形状对」）|
 *
 * # 为什么非 nil 也照实打
 *
 * 求值器打的是**占位** `0xc000000000`（它没有真地址）。native 有真地址，
 * 照实打比造一个假常量更诚实 —— 而这类行本来就不可能逐字节对撞（`print.go`
 * 只钉了 4 个 `nil` 形状）。
 */
static void put_hex(u64 v) {
    static const char d[] = "0123456789abcdef";
    if (v == 0) {
        write_all(FD_STDOUT, "0x0", 3);
        return;
    }
    char buf[18];
    u64 n = 0;
    while (v > 0) {
        buf[n++] = d[v & 0xF];
        v >>= 4;
    }
    write_all(FD_STDOUT, "0x", 2);
    while (n > 0) {
        n--;
        write_all(FD_STDOUT, &buf[n], 1);
    }
}

void rt_print_ptr(u64 p) { put_hex(p); }

void rt_print_iface(u64 tab, u64 data) {
    write_all(FD_STDOUT, "(", 1);
    put_hex(tab);
    write_all(FD_STDOUT, ",", 1);
    put_hex(data);
    write_all(FD_STDOUT, ")", 1);
}

void rt_print_slice(u64 p, u64 len, u64 cap) {
    write_all(FD_STDOUT, "[", 1);
    rt_print_u64(len);
    write_all(FD_STDOUT, "/", 1);
    rt_print_u64(cap);
    write_all(FD_STDOUT, "]", 1);
    put_hex(p);
}


/* ════════════════════════════════════════════════════════════════════
 * defer / panic / recover（`M9/Phase 5`，`R-9-18`）
 *
 * # 三条设计约束（都是「不做」比「做」重要）
 *
 * 1. **不 unwind**：`.framemap` 不在 ELF 里，栈上哪一格是指针**运行期不知道**，
 *    所以不能用 `longjmp` 跳过帧（那样漏扫根，后续 GC 会看到悬垂指针）。
 *    改成**返回码传播**：`rt_panic` 只置状态就返回，每个调用点之后查一次
 *    `rt_panicking()`，非 0 就跳本帧的 panic 出口（跑 defer 再 `ret`）。
 * 2. **defer 只存 funcval**：`defer f(args...)` 的实参由 build 打包进一个闭包，
 *    所以运行期不需要知道任何 ABI 布局 —— `rt_deferrun` 只做「调一个 funcval」。
 * 3. **跑 defer 期间挂起 panic**：被 defer 调用的函数是**正常执行**的（它也会
 *    return），所以它内部的 `rt_panicking` 检查必须是 0。挂起 + 跑完恢复。
 * ════════════════════════════════════════════════════════════════════ */

/* 一条已登记的 defer。`fp` 是登记时那个帧的 `x29`（`rt_deferrun` 按它配对）。 */
typedef struct DeferRec {
    struct DeferRec *next;
    void *fp;
    void *fv;
} DeferRec;

static DeferRec *g_defer_top;

/* 当前 panic 的**文本**（已拷进堆：展开期间源帧会消失）。 */
static i64 g_panicking;
static char *g_p_text;
static u64 g_p_len;

/* 跑 defer 时被**挂起**的那个 panic（`recover()` 取的就是它）。 */
static i64 g_saved_panicking;
static char *g_saved_text;
static u64 g_saved_len;
static i64 g_in_defer;

/* `recover()` 返回的接口用这个 desc（`string`，16 字节）。
 * M9 没有 reflect（`U-M9-04`），所以 `recover()` 的返回值只有「非 nil」与
 * 「按 `C9` §4.3 打印」两个可观察面 —— 给一个 string 接口两件事都成立。 */
static const TypeDesc g_panic_str_desc = {16, 8, 0x9e3779b9u, 6, 1, 0, 0, 0, 0};

void rt_defer_push(void *fp, void *fv) {
    DeferRec *r = rt_arena_alloc(sizeof(DeferRec), 8);
    r->next = g_defer_top;
    r->fp = fp;
    r->fv = fv;
    g_defer_top = r;
}

/* 把文本拷进堆再记状态。`p` 可能是帧上的临时缓冲，展开期间就没了。 */
static void panic_set(const char *p, u64 n) {
    char *t = rt_arena_alloc(n + 1, 8);
    for (u64 i = 0; i < n; i++) {
        t[i] = p[i];
    }
    t[n] = 0;
    g_p_text = t;
    g_p_len = n;
    g_panicking = 1;
}

i64 rt_panicking(void) { return g_panicking; }

/* 调一个 funcval：`{ fn, caps... }` 的首字是代码地址。 */
static void call_funcval(void *fv) {
    void (*fn)(void *) = (void (*)(void *))((void **)fv)[0];
    fn(fv);
}

/* 帧 `fp` 的 defer 全部逆序跑掉。
 *
 * `recover()` 看的是 `g_saved_*`：同一时刻「正在传播的 panic」只有一个，而
 * defer 里新起的 panic 要**取代**它 —— 所以正在传播的那个搬进 `g_saved_*`。 */
void rt_deferrun(void *fp) {
    while (g_defer_top != 0 && g_defer_top->fp == fp) {
        DeferRec *r = g_defer_top;
        g_defer_top = r->next;
        g_saved_panicking = g_panicking;
        g_saved_text = g_p_text;
        g_saved_len = g_p_len;
        g_panicking = 0;
        g_p_text = 0;
        g_p_len = 0;
        g_in_defer = 1;
        if (r->fv == 0) {
            /* `defer nilf()`：Go 在**调用时**才 panic（不是登记时，`defernil.go`
             * 整份文件钉的就是这件事）。文本与解释器的 `RtError::NilDeref` 一致。 */
            const char *m =
                "runtime error: invalid memory address or nil pointer dereference";
            u64 n = 0;
            while (m[n] != 0) {
                n++;
            }
            panic_set(m, n);
        } else {
            call_funcval(r->fv);
        }
        g_in_defer = 0;
        if (!g_panicking) {
            /* 没有新 panic ⇒ 把挂起的那个放回去（`recover` 过就放回 0） */
            g_panicking = g_saved_panicking;
            g_p_text = g_saved_text;
            g_p_len = g_saved_len;
        }
    }
}

/* `recover()`：写 16 字节接口到 `out`。 */
void rt_recover(void *out) {
    if (g_in_defer && g_saved_panicking) {
        void *box = rt_arena_alloc(16, 8);
        *(char **)box = g_saved_text;
        *(u64 *)((char *)box + 8) = g_saved_len;
        *(const void **)out = (const void *)&g_panic_str_desc;
        *(void **)((char *)out + 8) = box;
        g_saved_panicking = 0;
        g_saved_text = 0;
        g_saved_len = 0;
        return;
    }
    *(const void **)out = 0;
    *(void **)((char *)out + 8) = 0;
}

/* 没有人 recover 的 panic：`main` 的收尾调它（`C9` §4.5：一行到 stderr + 退出 2）。 */
void rt_print_panic(void) {
    write_all(FD_STDERR, "panic: ", 7);
    if (g_p_len > 0) {
        write_all(FD_STDERR, g_p_text, g_p_len);
    }
    write_all(FD_STDERR, "\n", 1);
}

/* ── panic 的九个入口（`C9` §4.5）──
 *
 * 它们**只置状态**，不退出：退出由 `main` 的收尾做（那样 `recover` 才有机会）。
 * 文本走「捕获」而不是直接写出去 —— 被 recover 掉就不该有输出。 */

/* 开始捕获：前缀 `panic: ` **不进**捕获（`rt_print_panic` 自己加）。 */
static void panic_prefix(void) {
    g_capturing = 1;
    g_cap_len = 0;
}

static void panic_commit(void) {
    g_capturing = 0;
    panic_set(g_cap_buf == 0 ? "" : g_cap_buf, g_cap_len);
}

void rt_panic_str(const char *p, u64 n) {
    panic_prefix();
    write_all(FD_STDERR, p, n);
    panic_commit();
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
    panic_commit();
}

/* 三个检查点的出口（`M7-design.md` §3.9）。文案与 Go 的对应 panic 一致。 */
void rt_panic_div_zero(void) {
    const char *m = "runtime error: integer divide by zero";
    u64 n = 0;
    while (m[n] != 0) {
        n++;
    }
    panic_set(m, n);
}

void rt_panic_nil(void) {
    const char *m =
        "runtime error: invalid memory address or nil pointer dereference";
    u64 n = 0;
    while (m[n] != 0) {
        n++;
    }
    panic_set(m, n);
}

/* 切片表达式的越界：`panic: runtime error: slice bounds out of range [k:v] with capacity c`
 *
 * # ⚠️ 这里的文本与**官方**不一致（与解释器一致）
 *
 * 官方 Go 打的是 `[:9]`（只写越界的那个界），而解释器打的是 `[slice:9]`
 * （`what` 是 `string` / `array` / `slice`）。M9 选择**跟解释器**：
 * `D` 腿（native vs 解释器）要逐字节比，两边不一致就永远红。
 * 解释器那侧改文本是 M5 的待办（记在 `G-M9-02`），改完这里跟着改一行。
 *
 * `kind`：0 = string、1 = array、2 = slice。 */
void rt_panic_slice_bounds(u64 val, u64 max, u64 kind) {
    panic_prefix();
    const char *a = "runtime error: slice bounds out of range [";
    u64 k = 0;
    while (a[k] != 0) {
        k++;
    }
    write_all(FD_STDERR, a, k);
    const char *w = (kind == 0) ? "string" : (kind == 1) ? "array" : "slice";
    u64 wn = 0;
    while (w[wn] != 0) {
        wn++;
    }
    write_all(FD_STDERR, w, wn);
    write_all(FD_STDERR, ":", 1);
    char buf[20];
    u64 len = 0;
    if (val == 0) {
        buf[len++] = '0';
    } else {
        char tmp[20];
        u64 t = 0;
        u64 v = val;
        while (v > 0) {
            tmp[t++] = (char)('0' + (v % 10));
            v /= 10;
        }
        while (t > 0) {
            buf[len++] = tmp[--t];
        }
    }
    write_all(FD_STDERR, buf, len);
    const char *b = "] with capacity ";
    k = 0;
    while (b[k] != 0) {
        k++;
    }
    write_all(FD_STDERR, b, k);
    len = 0;
    if (max == 0) {
        buf[len++] = '0';
    } else {
        char tmp[20];
        u64 t = 0;
        u64 v = max;
        while (v > 0) {
            tmp[t++] = (char)('0' + (v % 10));
            v /= 10;
        }
        while (t > 0) {
            buf[len++] = tmp[--t];
        }
    }
    write_all(FD_STDERR, buf, len);
    panic_commit();
}

/* 写 nil map：`panic: assignment to entry in nil map`（文本与解释器一致）。 */
void rt_panic_nilmap(void) {
    const char *m = "assignment to entry in nil map";
    u64 n = 0;
    while (m[n] != 0) {
        n++;
    }
    panic_set(m, n);
}

/* 越界：`panic: runtime error: index out of range [i] with length n`
 *
 * 文本与解释器**逐字节一致**（`C9` §4.5 / `rgoc-interp/src/ops.rs`）——
 * 这是 `D` 腿（native vs 解释器）能对上的前提。
 *
 * 打印数字自己拼（不用 `fmt_u64`）：那个函数定义在下面，而这里要一个
 * 「只往 stderr 写、不碰 stdout」的版本 —— 越界时 stdout 必须保持原样。 */
void rt_panic_bounds(u64 i, u64 n) {
    panic_prefix();
    const char *a = "runtime error: index out of range [";
    u64 k = 0;
    while (a[k] != 0) {
        k++;
    }
    write_all(FD_STDERR, a, k);
    char buf[20];
    u64 len = 0;
    if (i == 0) {
        buf[len++] = '0';
    } else {
        char tmp[20];
        u64 t = 0;
        u64 v = i;
        while (v > 0) {
            tmp[t++] = (char)('0' + (v % 10));
            v /= 10;
        }
        while (t > 0) {
            buf[len++] = tmp[--t];
        }
    }
    write_all(FD_STDERR, buf, len);
    const char *b = "] with length ";
    k = 0;
    while (b[k] != 0) {
        k++;
    }
    write_all(FD_STDERR, b, k);
    len = 0;
    if (n == 0) {
        buf[len++] = '0';
    } else {
        char tmp[20];
        u64 t = 0;
        u64 v = n;
        while (v > 0) {
            tmp[t++] = (char)('0' + (v % 10));
            v /= 10;
        }
        while (t > 0) {
            buf[len++] = tmp[--t];
        }
    }
    write_all(FD_STDERR, buf, len);
    panic_commit();
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

/* 浮点/复数的输出通道：`1` = stdout（`println`），`2` = stderr（`panic`）。
 *
 * `rt_print_f64` 里所有 `write_all` 都走这个变量 —— 否则要给「打印」和「panic」
 * 各写一份格式化，两份必然漂。单线程，不需要保护。 */
static i64 g_out_fd = FD_STDOUT;

/* `println` 一个浮点（`x0` = is32）。 */
void rt_print_f64(double v, i64 is32) {
    /* 通道由**入口**决定（`rt_panic_*` 设成 stderr 后直接 `exit`，
     * 所以这里不需要重置 —— 重置反而会把 panic 的通道改回 stdout）。 */
    u64 bits;
    /* 用位模式取值，避免任何隐式转换 */
    if (is32) {
        /* `v` 里放的是 float32 的值（低 32 位有效） */
        u32 b32 = (u32)(*(u64 *)&v);
        bits = (u64)b32;
        if (((b32 >> 23) & 0xff) == 0xff) {
            if ((b32 & 0x7fffff) != 0) {
                write_all(g_out_fd, "NaN", 3);
            } else {
                write_all(g_out_fd, (b32 >> 31) ? "-Inf" : "+Inf", (b32 >> 31) ? 4 : 4);
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
        write_all(g_out_fd, out, (u64)n);
        return;
    }
    bits = *(u64 *)&v;
    if (is_nan_f64(bits)) {
        write_all(g_out_fd, "NaN", 3);
        return;
    }
    if (is_inf_f64(bits)) {
        if (bits >> 63) {
            write_all(g_out_fd, "-Inf", 4);
        } else {
            write_all(g_out_fd, "+Inf", 4);
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
    write_all(g_out_fd, out, (u64)n);
}

/* `println` 复数：`(re+imi)`（`C9` §4.3）。 */
void rt_print_complex(double re, double im) {
    write_all(g_out_fd, "(", 1);
    rt_print_f64(re, 0);
    if (*(u64 *)&im >> 63) {
        write_all(g_out_fd, "-", 1);
        double a = -im;
        rt_print_f64(a, 0);
    } else {
        write_all(g_out_fd, "+", 1);
        rt_print_f64(im, 0);
    }
    write_all(g_out_fd, "i)", 2);
}

void rt_print_complex64(float re, float im) {
    write_all(g_out_fd, "(", 1);
    double dre = (double)re;
    rt_print_f64(dre, 1);
    if (im < 0.0f || (im == 0.0f && 1.0f / im < 0.0f)) {
        write_all(g_out_fd, "-", 1);
        double a = (double)(-im);
        rt_print_f64(a, 1);
    } else {
        write_all(g_out_fd, "+", 1);
        rt_print_f64((double)im, 1);
    }
    write_all(g_out_fd, "i)", 2);
}

/* `panic(float)`：与 `println` 同一份格式化，只是走 stderr（`C9` §5）。
 *
 * 直接把槽里的位模式当整数打（`rt_panic_i64`）会打出 `1107296256` 这种
 * 「看起来像个大整数」的东西 —— 静默错，而且很容易被当成真值。 */
void rt_panic_f64(double v, i64 is32) {
    panic_prefix();
    rt_print_f64(v, is32);
    panic_commit();
}

void rt_panic_complex(double re, double im) {
    panic_prefix();
    rt_print_complex(re, im);
    panic_commit();
}
