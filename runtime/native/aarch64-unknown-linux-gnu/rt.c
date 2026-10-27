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
#define SYS_getrusage 165

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
#define CHUNK_SHIFT 24UL
#define CHUNK_MIN (1UL << CHUNK_SHIFT) /* 16 MiB：`p >> 24` 就是 chunk id */
#define HEAP_LIMIT_DEFAULT (256UL * 1024UL * 1024UL)

/* 块头 16 字节（`D-M10-2` / `C5` §11.1）：`{header, n|kind}` 两行。 */
#define BLK_HDR 16UL
#define BLK_MIN 16UL /* 空闲块也有头 */

/* 帧记录区（`C12` §4.2，与 `rgoc_arm64::frame::REC_*` **同值**）。
 *
 * ```text
 * [fp-16] prev    上一帧的 fp（0 = 链尾）
 * [fp-24] fi      *RtFuncInfo
 * [fp-32] sp_idx  本帧当前的 safepoint 下标
 * ```
 *
 * ⚠️ 三个值必须与 Rust 侧一致：`_Static_assert` 做不到（那边是 `i32` 常量），
 * 所以靠 `T-ROOT-09`（帧记录链能跑通）与 `C12` §4.2 的文字钉。
 */
#define REC_PREV (-16)
#define REC_FI (-24)
#define REC_SP (-32)

/* ⚠️ **Chunk 头 64 字节**，与块头**不是一回事**。`bump` 从 `CHUNK_HDR` 起 ——
 * 写成 `BLK_HDR`（16）会让第一个块覆盖自己的 chunk 头（`size`/`bump`/
 * `free_head`/`meta_bm` 全被写坏，症状是第二次分配时 `meta_bm == 0` 段错误）。
 * 实测踩过：`off=16`、`bm=0`。 */
#define CHUNK_HDR 64UL

/* `header` 的 tag 位（活块）。desc 8 对齐 ⇒ 低 3 位恒 0。 */
#define HDR_FREE 1UL   /* 空闲块（此时 bit32..63 = 本块总字节数） */
#define HDR_MARK 2UL   /* 已标记 */
#define HDR_RAW 4UL    /* 不扫描（与 kind==K_RAW 同一件事的快速编码） */
#define HDR_DESC(h) ((const void *)((h) & ~7UL))

/* `+8` 的 kind（占高 32 位）。`K_ARENA` 是「永不回收、不扫描」的运行时结构。 */
enum { K_OBJECT = 0, K_ARRAY = 1, K_CLOSURE = 2, K_RAW = 3, K_MAPSLOTS = 4, K_ARENA = 5 };

/* map 槽状态（`C12` §2.2 的 `mapslots`）。**扫描器要用**，所以提到这里。
 *
 * `TOMB` 与 `EMPTY` 必须分开：`TOMB` **不终止探测** —— 否则「删过的键」会把
 * 后面的键挡在探测链外（查不到）。 */
#define MAP_EMPTY 0
#define MAP_LIVE 1
#define MAP_TOMB 2

/* 8 字节向上取整。定义在 map 那一节，但扫描器也要用（算槽的 stride）。 */
static u64 align8(u64 n);

/* 一个 chunk（`C12` §1.1）。**正好 64 字节**，在数据区**之内**（前 64 字节）。
 *
 * `base` 16 MiB 对齐 ⇒ 任意堆内地址 `p` 的 `p >> 24` 就是本 chunk 的 id
 * （`data` 容量 >= 16 MiB，多段 chunk 每段各登记一次）。
 */
typedef struct Chunk {
    struct Chunk *next;
    u64 base;      /* 16 MiB 对齐的数据区起点（含本头） */
    u64 size;      /* 数据区总容量（>= 16 MiB，16 MiB 的倍数） */
    u64 bump;      /* 下一个未分配偏移（相对 base），从 CHUNK_HDR 起 */
    u64 free_head; /* 空闲链头（相对 base 的偏移；0 = 无） */
    u8 *meta_bm;   /* 起始位图：1 位 / 8 字节（16 MiB ⇒ 256 KiB） */
    u8 *meta_sum;  /* 页摘要：1 位 / 4 KiB（16 MiB ⇒ 512 B） */
    u64 live_bytes;
} Chunk;

_Static_assert(sizeof(Chunk) == CHUNK_HDR, "Chunk 头必须是 64 字节（C12 §1.1）");

static Chunk *heap;
static u64 heap_committed; /* 已 mmap 的字节（含头与元数据），上限按它算 */
static u64 heap_limit = HEAP_LIMIT_DEFAULT;

/* 定义在本节之后（`rt_fatal_oom` 要用 `write_all`，而 `chunk_tab_put` 要用它）。 */
static void rt_fatal_oom(u64 limit);

/* `p >> 24` → Chunk 的 64 槽开放寻址表（`C12` §1.1）。
 *
 * # 为什么不用「遍历链」
 *
 * `mark_ptr` 对**每一个**根与每一个子指针都要先问「在堆里吗」。遍历链是
 * O(chunks)，而标记的总次数是 O(活对象数) —— 两者相乘。哈希表是 O(1)，
 * 而 64 槽足够（64 × 16 MiB = 1 GiB > 缺省上限 256 MiB）。
 */
typedef struct {
    u64 id;
    Chunk *c;
} ChunkSlot;

#define CHUNK_SLOTS 64UL
static ChunkSlot chunk_tab[CHUNK_SLOTS];

static u64 chunk_hash(u64 id) { return (id * 0x9E3779B97F4A7C15UL) >> 58; }

static void chunk_tab_put(u64 id, Chunk *c) {
    u64 h = chunk_hash(id);
    for (u64 i = 0; i < CHUNK_SLOTS; i++) {
        u64 k = (h + i) & (CHUNK_SLOTS - 1);
        if (chunk_tab[k].c == 0 || chunk_tab[k].id == id) {
            chunk_tab[k].id = id;
            chunk_tab[k].c = c;
            return;
        }
    }
    /* 64 槽全满：64 × 16 MiB = 1 GiB，而缺省上限是 256 MiB ⇒ 撞到这里说明
     * 有人把 `RGOC_HEAP_MAX` 调到 1 GiB 以上。当 OOM 处理（而不是静默漏登记，
     * 那会让 `mark_ptr` 认不出堆指针 ⇒ 活对象被回收 ⇒ 段错误）。 */
    rt_fatal_oom(heap_limit);
}

static Chunk *chunk_by_id(u64 id) {
    u64 h = chunk_hash(id);
    for (u64 i = 0; i < CHUNK_SLOTS; i++) {
        u64 k = (h + i) & (CHUNK_SLOTS - 1);
        if (chunk_tab[k].c == 0) {
            return 0;
        }
        if (chunk_tab[k].id == id) {
            return chunk_tab[k].c;
        }
    }
    return 0;
}

/* 这个地址在托管堆里吗？（`C12` §4.1 的 `mark_ptr` 第一步）
 *
 * 返回所在 chunk（`p` 可能指向块中部），不在堆里返回 0。 */
static Chunk *chunk_of(const void *p) {
    u64 a = (u64)p;
    Chunk *c = chunk_by_id(a >> CHUNK_SHIFT);
    if (c == 0) {
        return 0;
    }
    if (a < c->base + CHUNK_HDR || a >= c->base + c->size) {
        return 0;
    }
    return c;
}

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
/* ── GC 的四个运行期开关与统计量（M10/Task 3，`C12` §5）──
 *
 * # 为什么这些是**运行期**环境变量而不是编译期常量
 *
 * `T-CORP-05` 要求**同一个产物**在 `off` / `on` / `stress` 三种态下跑出同样的
 * 字节。编译期开关会让「三态一致」变成「三个二进制各自一致」—— 那测不出
 * 「GC 没改语义」，只测出「三次编译都没崩」。
 *
 * # `gc_on` / `gc_stress` / `gc_threshold` 在 Phase 1 之前没有消费点
 *
 * Phase 0 只立「统计行」这个观测面；`rt_gc()` 与 `gc_alloc()` 在 Phase 1 才出现。
 * 解析先做是因为**环境变量的名字与语义是契约**（`C12` §5.1），越早钉越好。
 */
static u64 gc_on = 1;                            /* 0 = 关（与 M9 等价） */
static u64 gc_stress;                            /* 1 = 每次分配前先收 */

/* 「这段时间里不要触发 GC」（`R-10-23`）。
 *
 * # 为什么需要
 *
 * 有些分配是**构造函数的一部分**：块刚分配出来、还没写进任何可达位置。
 * 如果这时 GC 跑起来，那块就是不可见的 ⇒ 被收掉。`rt_alloc_escaped` 就是
 * 这个形状（逃逸槽的存储在序言里分配，紧接着存进帧里的指针格）。
 *
 * 关掉触发是**安全**的：从分配到发布之间没有任何分配点，所以「这段时间里
 * 没有垃圾产生」—— 不收也不会 OOM（上限只在 `rt_grow` 里判）。
 *
 * 与 `gc_pin` 的分工：`gc_pin` 解决「中间还要再分配一次」（map 的两块），
 * `gc_pause` 解决「一次分配 + 立即发布」。 */
static u64 gc_pause;
static u64 gc_stats;                             /* 1 = 退出时打一行 */
/* M11a 的 `rt_switch` 自测开关（`T-STK-01`）。缺省关：它切一次栈，
 * 对正常程序没有意义，只在判据里开。 */
static u64 g11_selftest;
/* 调度骨架的自测开关（`GO-2` 第一条）。 */
static u64 g11_schedtest;
/* `rt_go` 的自测开关（Task 12）。 */
static u64 g11_gotest;
/* `rt_send`/`rt_recv` 直通路径的自测开关（Task 17）。 */
static u64 g11_chan2test;
/* 阻塞 + 唤醒的自测开关（Task 18）。 */
static u64 g11_chan3test;
static u64 gc_threshold = 4UL * 1024UL * 1024UL; /* 自动触发阈值 */
/* 上次 GC 之后**分配的数据字节**（`C12` §5.3）。自动触发看它。 */
static u64 gc_since;

/* 统计量（`C12` §5.2 的 11 个字段）。Phase 0 全是 0，逐 Task 填实。 */
static u64 gc_collections;   /* 收了几次 */
static u64 gc_allocs;        /* 分配了几次 */
static u64 gc_frees;         /* 回收了几块 */
static u64 gc_live_bytes;    /* 上次清扫后的存活块字节（含 16 字节头） */
static u64 gc_alloc_bytes;   /* 累计分配的 data 字节（不含头） */
static u64 gc_kind_count[5]; /* object / array / closure / raw / mapslots */
static u64 gc_bad_sp;        /* sp_idx >= n_sp 的次数（必须 0） */
static u64 gc_bad_ptr;       /* 堆内指针解析不到块首的次数（必须 0） */
static u64 gc_bad_desc;      /* 块头 desc 非法的次数（必须 0） */

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

/* ════════════════════════════════════════════════════════════════════
 * goroutine 描述符与栈（M11a/Task 8，`M11-design.md` §3.2）
 * ════════════════════════════════════════════════════════════════════
 *
 * # `D-M11-2`：每个 G 一块**固定**栈，`mmap` 拿
 *
 * 64 KiB 是**实测钉住**的数字，不是估算（`M11-design.md` §2.4 ① 修正过一版：
 * 初稿写「帧深上限 1000 × 每帧几百字节 ⇒ 够 1000 层」，算一下就知道不成立）。
 * 溢出深度由 `T-STK-06` 实测并进 manifest。
 *
 * # `D-M11-3`：切换保存区压在**被挂起 G 自己的栈**上
 *
 * 20 个 8 字节槽（`x19..x28` / `x29` / `x30` / `d8..d15`）= 160 字节。
 * 存进 `gdesc.regs[]` 的话 GC 就必须扫寄存器槽 ⇒ `t_gc_10`（没有寄存器根）
 * 直接失效 —— 而那条守卫**不许为 M11 改**。
 */
#define G_STACK_BYTES (64 * 1024)
#define G_SWITCH_SAVE 160

/* `C5` §7.4：序言 `adrp/add/ldr` 它做栈下界比较。
 * ⚠️ **全局而不是寄存器** —— 用寄存器会让 `t_gc_10` 红。 */
u64 *g_stack_lo;

typedef struct GDesc {
    u64 *sp;             /* +0  挂起时的栈顶（切换点） */
    u64 *stack_lo;       /* +8  栈低地址 */
    u64 *stack_hi;       /* +16 栈高地址（初始 sp） */
    void *frames;        /* +24 本 G 的 g_frames 链头（M11c 起用） */
    void *wait;          /* +32 select 的 WaitSet（阻塞时） */
    struct GDesc *qnext; /* +40 就绪队列后继 */
    u32 state;           /* +48 0=ready 1=running 2=blocked 3=exited */
    u32 id;              /* +52 单调递增（调试与确定性） */
    u64 pad;             /* +56 */
} GDesc;                 /* 64 字节 */

/* `M11-design.md` §4.4：goroutine 数的硬上限。 */
#define G_MAX 1024

/* ⚠️ **静态数组，不走 `rt_arena_alloc`**。
 *
 * 两个理由，第二个是硬的：
 *
 * ① 设计里说「`gdesc` 放 arena（永不回收，量小）」—— 静态数组是同一效果；
 * ② **`rt_init` 会调它**。走 arena 就等于在 `_start` 里触发第一次 `rt_grow`
 *    （16 MiB 的 mmap），于是「空程序也只在第一次分配时才 mmap」这条 M10 的
 *    惰性行为变了 ⇒ `allocs` / `heap` 的读数全部偏移，会打破 M10 的判据。
 *    64 KiB 的 `.bss` 不进文件、也不占物理页（碰过才占）。
 */
static GDesc g_descs[G_MAX];
static GDesc *g_cur;
static u32 g_next_id;
static u32 g_count;

/* 分配一个**空的** gdesc（栈由调用方填）。
 *
 * 上限检查在 Task 29（`runtime error: too many goroutines (limit 1024)`）；
 * M11a 只有 `main` 一个 G，走不到那里。 */
static GDesc *rt_gdesc_new(void) {
    if (g_count >= G_MAX) {
        /* Task 29 会换成 `C9` §5 的文本 + 退出 2。 */
        rt_fatal_oom(heap_limit);
    }
    GDesc *g = &g_descs[g_count];
    g_count++;
    /* 静态数组只在 `.bss` 里清零一次，复用的槽要手动清。 */
    *g = (GDesc){0};
    g->id = ++g_next_id;
    return g;
}

/* 给一个 gdesc 配一块 `mmap` 的栈（`D-M11-2`）。
 *
 * 返回值仍是那个 gdesc；`sp` 指向栈顶（**16 对齐**，AAPCS64 要求）。 */
static void rt_gstack_alloc(GDesc *g) {
    u64 total = G_STACK_BYTES + 16;
    i64 r = sys6(SYS_mmap, 0, (i64)total, PROT_READ_WRITE, MAP_PRIVATE_ANON, -1, 0);
    if (r < 0) {
        rt_fatal_oom(heap_limit);
    }
    /* 栈顶**向下**对齐：`mmap` 返回页对齐，加 total 之后可能不是 16 的倍数。 */
    u64 hi = ((u64)r + total) & ~(u64)15;
    u64 lo = hi - G_STACK_BYTES;
    g->stack_hi = (u64 *)hi;
    g->stack_lo = (u64 *)lo;
    g->sp = (u64 *)hi;
}

/* 切到 `g`（只更新「当前 G」与序言读的那个下界）。
 *
 * ⚠️ **不切 `sp`** —— `sp` 的切换是 `rt_switch` 的事（Task 9）。这里只维护
 * 元数据，所以它可以在 `_start` 里安全调用。 */
static void rt_gset_current(GDesc *g) {
    g_cur = g;
    g_stack_lo = g->stack_lo;
}

/* ════════════════════════════════════════════════════════════════════
 * 上下文切换（M11a/Task 9，`M11-design.md` §3.3 / `D-M11-3`）
 * ════════════════════════════════════════════════════════════════════
 *
 * `void rt_switch(u64 **save_sp, u64 *new_sp)`
 *
 * # 为什么保存区是 160 字节
 *
 * `x19..x28`（10）+ `x29` + `x30` + `d8..d15`（8）= 20 个 8 字节槽。
 * AAPCS64 规定 `x19..x28` / `v8..v15` 是 callee-saved，而 `rt_switch` 是
 * 被 `bl` **调用**的 —— 不保存就违反 ABI，G 之间会串味（`C5` §6.1 的注）。
 *
 * # 为什么压进**被挂起 G 自己的栈**而不是 `gdesc.regs[]`
 *
 * 存进 `gdesc` 的话 GC 就必须扫那 20 个槽 ⇒ `t_gc_10`（没有寄存器根）失效，
 * 而那条守卫**不许为 M11 改**。压在栈上则「值仍然全在栈里」⇒ M10 的根协议
 * 一个字都不用动（`M11-design.md` §2.2 的方案 B）。
 *
 * # 为什么用顶层 `__asm__` 而不是内联汇编
 *
 * 内联汇编的 clobber 列表要写全 20 个寄存器，而编译器仍可能在函数序言/尾声
 * 插入对它们的访问 —— 那种错误只在开了优化时露。顶层汇编块是**整个函数**，
 * 没有这个问题。
 */
void rt_switch(u64 **save_sp, u64 *new_sp);

__asm__(
    ".text\n"
    ".globl rt_switch\n"
    ".type rt_switch, %function\n"
    ".balign 4\n"
    "rt_switch:\n"
    "    sub  sp, sp, #160\n"
    "    stp  x19, x20, [sp, #0]\n"
    "    stp  x21, x22, [sp, #16]\n"
    "    stp  x23, x24, [sp, #32]\n"
    "    stp  x25, x26, [sp, #48]\n"
    "    stp  x27, x28, [sp, #64]\n"
    "    stp  x29, x30, [sp, #80]\n"
    "    stp  d8,  d9,  [sp, #96]\n"
    "    stp  d10, d11, [sp, #112]\n"
    "    stp  d12, d13, [sp, #128]\n"
    "    stp  d14, d15, [sp, #144]\n"
    "    mov  x9, sp\n"
    "    str  x9, [x0]\n"
    "    mov  sp, x1\n"
    "    ldp  x19, x20, [sp, #0]\n"
    "    ldp  x21, x22, [sp, #16]\n"
    "    ldp  x23, x24, [sp, #32]\n"
    "    ldp  x25, x26, [sp, #48]\n"
    "    ldp  x27, x28, [sp, #64]\n"
    "    ldp  x29, x30, [sp, #80]\n"
    "    ldp  d8,  d9,  [sp, #96]\n"
    "    ldp  d10, d11, [sp, #112]\n"
    "    ldp  d12, d13, [sp, #128]\n"
    "    ldp  d14, d15, [sp, #144]\n"
    "    add  sp, sp, #160\n"
    "    ret\n"
    ".size rt_switch, .-rt_switch\n");

/* ── 自测装置（`T-STK-01`）────────────────────────────────────────────
 *
 * 目标：切出去、再切回来，`x19..x28` 与 `d8..d15` 逐个不变。
 *
 * 为什么必须用汇编验：C 语言**看不到**寄存器分配，编译器可以随时复用它们。
 * 所以「填值 + 比对」只能是汇编，「切换」也只能是汇编 —— 只有「构造 B 栈」
 * 与「判成败」可以用 C。
 *
 * B 栈的构造（`rt_switch` 的契约决定）：
 *
 * ```text
 *   [sa + 0 .. sa + 160)   20 个槽：x19,x20,…,x29,x30,d8,…,d15
 *   sa[11] = rt_sc_b       ← 恢复后 `ret` 跳这里（x30 在 #88 = 槽 11）
 *   sa + 160 = B 的初始 sp
 * ```
 */
#define SC_STACK_WORDS 128 /* 1 KiB，够一个保存区 + 余量 */

/* ⚠️ 这几个**不能** `static`：下面的顶层汇编块要用 `adrp/add :lo12:` 直接
 * 引用它们，而 `static` 符号在 `.o` 里是 local —— 汇编块引用不到（实测
 * `undefined reference to sc_save_a`）。
 *
 * ⚠️ **必须 16 对齐**：`rt_switch` 用 `stp/ldp` 以 `sp` 为基址，而 AAPCS64
 * 要求那类访问的 `sp` **16 字节对齐**。`u64[]` 默认只保证 8 —— 实测不写
 * `aligned(16)` 会段错误（`rc=139`）。`SC_STACK_WORDS` 是 128（1024 B，16 的
 * 倍数），所以 `top = base + 128`、`sa = top - 20` 也都是 16 对齐。 */
u64 sc_b_stack[SC_STACK_WORDS] __attribute__((aligned(16)));
u64 *sc_b_sp;
u64 *sc_save_a;
u64 *sc_save_b;
i64 sc_fail;

void rt_sc_run(void);
void rt_sc_b(void);

i64 rt_switch_selftest(void) {
    u64 *top = sc_b_stack + SC_STACK_WORDS;
    u64 *sa = top - 20;
    for (u64 i = 0; i < 20; i++) {
        sa[i] = 0;
    }
    sa[11] = (u64)rt_sc_b; /* x30 */
    sc_b_sp = sa;
    sc_fail = 0;
    rt_sc_run();
    return sc_fail;
}

__asm__(
    ".text\n"
    /* B 的入口：切回 A，然后把 A 的 `rt_switch` 之后的代码接着跑。 */
    ".globl rt_sc_b\n"
    ".type rt_sc_b, %function\n"
    ".balign 4\n"
    "rt_sc_b:\n"
    "    adrp x0, sc_save_b\n"
    "    add  x0, x0, :lo12:sc_save_b\n"
    "    adrp x1, sc_save_a\n"
    "    add  x1, x1, :lo12:sc_save_a\n"
    "    ldr  x1, [x1]\n"
    "    bl   rt_switch\n"
    "    brk  #1\n" /* 不该回来 */
    ".size rt_sc_b, .-rt_sc_b\n"
    /* 填可区分值 → 切到 B → 切回来 → 逐个比对。失败置 sc_fail = 1。 */
    ".globl rt_sc_run\n"
    ".type rt_sc_run, %function\n"
    ".balign 4\n"
    "rt_sc_run:\n"
    "    stp  x29, x30, [sp, #-16]!\n"
    "    mov  x29, sp\n"
    /* ⚠️ **必须先把 C 的 callee-saved 存起来**（`x19..x28` + `d8..d15`）。
     *
     * 这个函数要往这 18 个寄存器里填「可区分值」再比对 —— 而它们按 AAPCS64
     * 是 **callee-saved**：不保存就返回，调用方（C）用它们存的局部变量会全
     * 变成 0x1111 这类垃圾。实测症状是 `rc=139`（C 的返回地址被踩），
     * 而且**看起来像是 `rt_switch` 切坏了** —— 实际是自测函数自己没守 ABI。 */
    "    sub  sp, sp, #160\n"
    "    stp  x19, x20, [sp, #0]\n"
    "    stp  x21, x22, [sp, #16]\n"
    "    stp  x23, x24, [sp, #32]\n"
    "    stp  x25, x26, [sp, #48]\n"
    "    stp  x27, x28, [sp, #64]\n"
    "    stp  d8,  d9,  [sp, #80]\n"
    "    stp  d10, d11, [sp, #96]\n"
    "    stp  d12, d13, [sp, #112]\n"
    "    stp  d14, d15, [sp, #128]\n"
    /* 填 x19..x28：0x1111, 0x2222, …, 0xAAAA */
    "    movz x19, #0x1111\n"
    "    movz x20, #0x2222\n"
    "    movz x21, #0x3333\n"
    "    movz x22, #0x4444\n"
    "    movz x23, #0x5555\n"
    "    movz x24, #0x6666\n"
    "    movz x25, #0x7777\n"
    "    movz x26, #0x8888\n"
    "    movz x27, #0x9999\n"
    "    movz x28, #0xAAAA\n"
    /* 填 d8..d15：把整数位模式直接装进浮点寄存器（不经浮点运算） */
    "    movz x9, #0x0101\n"
    "    fmov d8, x9\n"
    "    movz x9, #0x0202\n"
    "    fmov d9, x9\n"
    "    movz x9, #0x0303\n"
    "    fmov d10, x9\n"
    "    movz x9, #0x0404\n"
    "    fmov d11, x9\n"
    "    movz x9, #0x0505\n"
    "    fmov d12, x9\n"
    "    movz x9, #0x0606\n"
    "    fmov d13, x9\n"
    "    movz x9, #0x0707\n"
    "    fmov d14, x9\n"
    "    movz x9, #0x0808\n"
    "    fmov d15, x9\n"
    /* 切到 B，再从 B 切回来 */
    "    adrp x0, sc_save_a\n"
    "    add  x0, x0, :lo12:sc_save_a\n"
    "    adrp x1, sc_b_sp\n"
    "    add  x1, x1, :lo12:sc_b_sp\n"
    "    ldr  x1, [x1]\n"
    "    bl   rt_switch\n"
    /* 回来了：逐个比对（x9/x10 是 caller-saved，可以随便用） */
    "    movz x9, #0x1111\n"
    "    cmp  x19, x9\n"
    "    b.ne .Lsc_bad\n"
    "    movz x9, #0x2222\n"
    "    cmp  x20, x9\n"
    "    b.ne .Lsc_bad\n"
    "    movz x9, #0x3333\n"
    "    cmp  x21, x9\n"
    "    b.ne .Lsc_bad\n"
    "    movz x9, #0x4444\n"
    "    cmp  x22, x9\n"
    "    b.ne .Lsc_bad\n"
    "    movz x9, #0x5555\n"
    "    cmp  x23, x9\n"
    "    b.ne .Lsc_bad\n"
    "    movz x9, #0x6666\n"
    "    cmp  x24, x9\n"
    "    b.ne .Lsc_bad\n"
    "    movz x9, #0x7777\n"
    "    cmp  x25, x9\n"
    "    b.ne .Lsc_bad\n"
    "    movz x9, #0x8888\n"
    "    cmp  x26, x9\n"
    "    b.ne .Lsc_bad\n"
    "    movz x9, #0x9999\n"
    "    cmp  x27, x9\n"
    "    b.ne .Lsc_bad\n"
    "    movz x9, #0xAAAA\n"
    "    cmp  x28, x9\n"
    "    b.ne .Lsc_bad\n"
    "    fmov x9, d8\n"
    "    movz x10, #0x0101\n"
    "    cmp  x9, x10\n"
    "    b.ne .Lsc_bad\n"
    "    fmov x9, d9\n"
    "    movz x10, #0x0202\n"
    "    cmp  x9, x10\n"
    "    b.ne .Lsc_bad\n"
    "    fmov x9, d10\n"
    "    movz x10, #0x0303\n"
    "    cmp  x9, x10\n"
    "    b.ne .Lsc_bad\n"
    "    fmov x9, d11\n"
    "    movz x10, #0x0404\n"
    "    cmp  x9, x10\n"
    "    b.ne .Lsc_bad\n"
    "    fmov x9, d12\n"
    "    movz x10, #0x0505\n"
    "    cmp  x9, x10\n"
    "    b.ne .Lsc_bad\n"
    "    fmov x9, d13\n"
    "    movz x10, #0x0606\n"
    "    cmp  x9, x10\n"
    "    b.ne .Lsc_bad\n"
    "    fmov x9, d14\n"
    "    movz x10, #0x0707\n"
    "    cmp  x9, x10\n"
    "    b.ne .Lsc_bad\n"
    "    fmov x9, d15\n"
    "    movz x10, #0x0808\n"
    "    cmp  x9, x10\n"
    "    b.ne .Lsc_bad\n"
    "    b    .Lsc_done\n"
    ".Lsc_bad:\n"
    "    adrp x9, sc_fail\n"
    "    add  x9, x9, :lo12:sc_fail\n"
    "    mov  x10, #1\n"
    "    str  x10, [x9]\n"
    ".Lsc_done:\n"
    /* 恢复 C 的 callee-saved（**必须**，见上面的注） */
    "    ldp  x19, x20, [sp, #0]\n"
    "    ldp  x21, x22, [sp, #16]\n"
    "    ldp  x23, x24, [sp, #32]\n"
    "    ldp  x25, x26, [sp, #48]\n"
    "    ldp  x27, x28, [sp, #64]\n"
    "    ldp  d8,  d9,  [sp, #80]\n"
    "    ldp  d10, d11, [sp, #96]\n"
    "    ldp  d12, d13, [sp, #112]\n"
    "    ldp  d14, d15, [sp, #128]\n"
    "    add  sp, sp, #160\n"
    "    ldp  x29, x30, [sp], #16\n"
    "    ret\n"
    ".size rt_sc_run, .-rt_sc_run\n");

/* ════════════════════════════════════════════════════════════════════
 * 调度骨架（M11a/Task 11，`M11-design.md` §3.5 / `C11` §2.1）
 * ════════════════════════════════════════════════════════════════════
 *
 * 三条规则（`GO-2`）：**M:1**（一个 OS 线程）/ **不抢占**（`go` 不切走当前 G）/
 * **就绪队列 FIFO**（队尾入、队头出）。
 *
 * 「不抢占」的落地很直接：`rt_schedule` 只在 `rt_goexit`（G 跑完）与阻塞点
 * （`ch <-` / `<-ch` / `select` 无就绪）被调用。没有任何地方按时间片切。
 */
static GDesc *g_ready_head;
static GDesc *g_ready_tail;

/* 入**队尾**（`GO-2` 的 FIFO）。改成队头插入就是变异 `M-M11-06`。 */
static void rt_enqueue(GDesc *g) {
    g->state = 0; /* ready */
    g->qnext = 0;
    if (g_ready_tail == 0) {
        g_ready_head = g;
        g_ready_tail = g;
    } else {
        g_ready_tail->qnext = g;
        g_ready_tail = g;
    }
}

static GDesc *rt_dequeue(void) {
    GDesc *g = g_ready_head;
    if (g == 0) {
        return 0;
    }
    g_ready_head = g->qnext;
    if (g_ready_head == 0) {
        g_ready_tail = 0;
    }
    g->qnext = 0;
    return g;
}

/* 死锁（`C9` §5 的文本，逐字节）。
 *
 * ⚠️ Task 26 会补「三条条件各自必要」的判定（`T-EXIT-05`：有就绪 G 时**不**
 * 能报死锁）。现在只有「队列空」这一条 —— M11a 还没有阻塞点，走不到这里。 */
static void rt_fatal_deadlock(void) {
    const char *m = "fatal error: all goroutines are asleep - deadlock!\n";
    u64 n = 0;
    while (m[n] != 0) {
        n++;
    }
    rt_write(2, m, n);
    rt_exit(2);
}

/* 换到下一个可跑的 G。
 *
 * 前置：当前 G **已经**入队（ready）、已 blocked、或已 exited —— 否则它的
 * `sp` 就丢了（`rt_switch` 会把当前 sp 存进 `prev->sp`，但没人再读它）。
 */
void rt_schedule(void) {
    GDesc *next = rt_dequeue();
    if (next == 0) {
        rt_fatal_deadlock();
    }
    GDesc *prev = g_cur;
    rt_gset_current(next);
    next->state = 1; /* running */
    rt_switch(&prev->sp, next->sp);
    /* 这里返回时，说明别的 G 切回了我 —— `g_cur` 已经由对方设好。 */
}

/* 当前 G 结束。**永不返回**。
 *
 * 不释放栈：M11a 没有 G 的回收（`rt_goexit` 只标记 exited）。栈的回收要等
 * M11c 之后 —— 现在 `mmap` 的 64 KiB 在进程退出时由内核收回。 */
void rt_goexit(void) {
    g_cur->state = 3; /* exited */
    rt_schedule();
    rt_exit(9); /* 走不到 */
}

/* ── 调度骨架的自测（`GO-2` 的第一条）────────────────────────────────
 *
 * 手工建两个 G（A 打印 `A`、B 打印 `B`），各跑完就 `rt_goexit`；`main` 自己
 * 也入队，跑完两个再回来。
 *
 * 期望输出恰好 `A\nB\n` —— **顺序**才是判据：倒序说明队头插入，乱序说明真的
 * 并发了（`M11-design.md` §3.5）。这条在 Task 12 有 `go` 语句之后会被
 * `t_go_03` 取代（那时用 Go 源码写同样的形状）。
 */
static void sc_g_a(void) {
    rt_write(1, "A\n", 2);
    rt_goexit();
}
static void sc_g_b(void) {
    rt_write(1, "B\n", 2);
    rt_goexit();
}

void rt_sc_tramp(void);

static GDesc *sc_mkg(void (*fn)(void)) {
    GDesc *g = rt_gdesc_new();
    rt_gstack_alloc(g);
    /* 在新栈顶构造 `rt_switch` 的入口状态：20 个槽 + `x30` = trampoline。
     * `x19` 装「要跑的函数」—— `rt_sc_tramp` 第一件事就是 `blr x19`。 */
    u64 *sa = g->stack_hi - 20;
    for (u64 i = 0; i < 20; i++) {
        sa[i] = 0;
    }
    sa[0] = (u64)fn;           /* x19 */
    sa[11] = (u64)rt_sc_tramp; /* x30 */
    g->sp = sa;
    return g;
}

void rt_schedtest(void) {
    rt_enqueue(sc_mkg(sc_g_a));
    rt_enqueue(sc_mkg(sc_g_b));
    rt_enqueue(g_cur); /* main 也在队列里 */
    rt_schedule();
}

__asm__(
    ".text\n"
    ".globl rt_sc_tramp\n"
    ".type rt_sc_tramp, %function\n"
    ".balign 4\n"
    "rt_sc_tramp:\n"
    "    mov  x0, x19\n"
    "    blr  x0\n"
    "    bl   rt_goexit\n"
    "    brk  #1\n" /* 不该回来 */
    ".size rt_sc_tramp, .-rt_sc_tramp\n");

/* ════════════════════════════════════════════════════════════════════
 * `go` 语句的运行时（M11a/Task 12，`M11-design.md` §3.6）
 * ════════════════════════════════════════════════════════════════════
 *
 * 新 G 的栈布局（从低到高）：
 *
 * ```text
 *   sa + 0   .. sa + 160     rt_switch 的 20 个保存槽
 *   sa + 160 .. sa + 224     实参区（x0..x7 各一格，共 64 字节）
 *   sa + 224                 fn
 *   sa + 232                 pad（让 sp 在 trampoline 里 16 对齐）
 *   sa + 240 = stack_hi      栈顶
 * ```
 *
 * `rt_switch` 切到 `sa` 后会 `add sp, sp, #160` ⇒ `sp = sa + 160`（实参区起点），
 * 然后 `ret` 跳到 `x30`（`rt_go_trampoline`）。
 *
 * # 为什么实参区固定在栈顶而不是「跟普通调用的 outgoing 区同一套」
 *
 * 普通调用的 outgoing 区在**调用方**帧尾，被调方从 `[sp, #0]` 开始读 —— 两者
 * 的**读法**完全一样（8 字节一格）。这里只是把那个区放在**新 G 的栈顶**，所以
 * trampoline 的 `ldp` 与任何被调方的序言是同一个偏移。
 *
 * # ⚠️ M11a 的窄化：只支持 ≤ 8 个标量实参
 *
 * 聚合实参按 `C5` §6.1 是**指针**（指向调用方帧里的副本）—— 而那个帧在 `go`
 * 之后可能被复用。深拷贝它要递归知道哪个格是指针，trampoline 不知道。
 * 所以 `isel` 只接受 ≤ 8 个标量（`SSA-UNSUPPORTED-GO-ARGS`），这一条进
 * `M11-report.md` 的窄化清单。
 */
#define G_GO_ARGBYTES 64
#define G_GO_FRAME (G_SWITCH_SAVE + G_GO_ARGBYTES + 16) /* 240 */

void rt_go_trampoline(void);

void rt_go(void *fn, const void *args, u64 argbytes) {
    GDesc *g = rt_gdesc_new();
    rt_gstack_alloc(g);
    u64 *sa = (u64 *)((u8 *)g->stack_hi - G_GO_FRAME);
    for (u64 i = 0; i < 20; i++) {
        sa[i] = 0;
    }
    sa[11] = (u64)rt_go_trampoline; /* x30 */
    u64 *a = sa + 20;               /* 实参区 */
    for (u64 i = 0; i < 8; i++) {
        a[i] = 0;
    }
    u64 n = argbytes > G_GO_ARGBYTES ? G_GO_ARGBYTES : argbytes;
    if (n > 0) {
        rt_memcpy(a, args, n);
    }
    *(u64 *)(sa + 28) = (u64)fn; /* 实参区之后就是 fn */
    g->sp = sa;
    /* 入队**队尾**，不切走当前 G（`C11` §2.1 的「不抢占」）。 */
    rt_enqueue(g);
}

__asm__(
    ".text\n"
    ".globl rt_go_trampoline\n"
    ".type rt_go_trampoline, %function\n"
    ".balign 4\n"
    "rt_go_trampoline:\n"
    /* sp 指向实参区（`rt_switch` 的 `add sp, #160` 之后） */
    "    ldp  x0, x1, [sp, #0]\n"
    "    ldp  x2, x3, [sp, #16]\n"
    "    ldp  x4, x5, [sp, #32]\n"
    "    ldp  x6, x7, [sp, #48]\n"
    "    ldr  x9, [sp, #64]\n" /* fn */
    /* 跳到栈顶（实参区 + fn + pad = 80），再 `blr` —— 被调方的 `stp x29,x30`
     * 才有地方落。80 是 16 的倍数 ⇒ 对齐不破。 */
    "    add  sp, sp, #80\n"
    "    blr  x9\n"
    "    bl   rt_goexit\n" /* 被调方返回 ⇒ 本 G 结束 */
    "    brk  #1\n"
    ".size rt_go_trampoline, .-rt_go_trampoline\n");

/* `rt_go` 的自测（Task 12；`go` 语句在 Task 13 才放开）。
 *
 * 用调度骨架那两个 C 函数当被调方 —— 它们签名是 `void(void)`，`argbytes = 0`。
 * 期望输出恰好 `A\n`。 */
void rt_gotest(void) {
    rt_go((void *)sc_g_a, 0, 0);
    rt_enqueue(g_cur);
    rt_schedule();
}

/* `key` **含**结尾的 `=`；返回它后面那个数字。
 *
 * # 为什么要有这个（而不是继续写 `parse_u64(e + N)`
 *
 * 硬编码偏移已经**数错两次**：`RGOC_G11_SCHEDTEST=` 写成 20（实际 19）、
 * `RGOC_G11_GOTEST=` 写成 15（实际 16）。两次的症状一样 —— 读到 `=` 或 `\0`，
 * `parse_u64` 返回 0，开关**静默失效**（自测不跑，看起来像「通过了」）。
 * 用 `strlen` 算就再也不会错。 */
static u64 env_u64(const char *e, const char *key) {
    u64 n = 0;
    while (key[n] != 0) {
        n++;
    }
    return parse_u64(e + n);
}


/* 定义在文件后半（要用 `Hchan`，而它在 `TypeDesc` 之后）。 */
void rt_chan2test(void);
void rt_chan3test(void);

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
        } else if (starts_with(e, "RGOC_GC=")) {
            /* `0` = 关；其余（含 `1`、空值）都是开。注意它**不是**
             * `RGOC_GC_STRESS=` 的前缀匹配 —— 第 8 个字符是 `_` 不是 `=`。 */
            if (e[8] == '0' && e[9] == 0) {
                gc_on = 0;
            }
        } else if (starts_with(e, "RGOC_GC_STRESS=")) {
            gc_stress = parse_u64(e + 15) != 0;
        } else if (starts_with(e, "RGOC_GC_STATS=")) {
            gc_stats = parse_u64(e + 14) != 0;
        } else if (starts_with(e, "RGOC_GC_THRESHOLD=")) {
            /* `0` 有意义（= 每次分配失败都收），所以不设下限。 */
            gc_threshold = parse_u64(e + 18);
        } else if (starts_with(e, "RGOC_G11_SELFTEST=")) {
            g11_selftest = env_u64(e, "RGOC_G11_SELFTEST=") != 0;
        } else if (starts_with(e, "RGOC_G11_SCHEDTEST=")) {
            g11_schedtest = env_u64(e, "RGOC_G11_SCHEDTEST=") != 0;
        } else if (starts_with(e, "RGOC_G11_GOTEST=")) {
            g11_gotest = env_u64(e, "RGOC_G11_GOTEST=") != 0;
        } else if (starts_with(e, "RGOC_G11_CHAN2TEST=")) {
            g11_chan2test = env_u64(e, "RGOC_G11_CHAN2TEST=") != 0;
        } else if (starts_with(e, "RGOC_G11_CHAN3TEST=")) {
            g11_chan3test = env_u64(e, "RGOC_G11_CHAN3TEST=") != 0;
        }
    }

    /* ── M11a：`main` 也是一个 G（Task 8）───────────────────────────
     *
     * `main` 用**进程栈**（内核给的 8 MiB），不是我们 `mmap` 的 64 KiB ——
     * `_start` 的 `sp` 已经在用它了，换栈要改生成的汇编（`M11-design.md`
     * §3.2 的图描述的是新 G 的栈）。
     *
     * `stack_lo` 取「入口 `sp` 往下 **4 MiB**」，与内核默认 `rlimit`（8 MiB）
     * **留一半余量**。
     *
     * ⚠️ 为什么不是 8 MiB（实测踩过）：`sp - 8 MiB` 正好落在栈底之外 ——
     * 检查**来不及**触发，进程先踩到 guard page ⇒ `rc=139`，而判据期望的是
     * `rc=2` + `goroutine stack exceeded`。取一半余量后，检查在还剩 4 MiB
     * 时就会报错，既不会误报（正常程序用不到 4 MiB）也不会漏报。
     *
     * 这是**近似**（`ulimit -s` 可变），但比「不检查」好得多。精确的溢出深度
     * 由 `T-STK-06` 在**新 G 的 64 KiB 栈**上实测钉住 —— 那条才是判据。 */
    {
        GDesc *m = rt_gdesc_new();
        m->sp = (u64 *)sp;
        m->stack_hi = (u64 *)sp;
        m->stack_lo = (u64 *)(sp - (4UL * 1024UL * 1024UL));
        m->state = 1; /* running */
        rt_gset_current(m);
    }

    /* M11a 自测（`T-STK-01`）：`RGOC_G11_SELFTEST=1` 时验一遍 `rt_switch`
     * 的 callee-saved 对称性。失败退出 **9**（不是 2 —— 那是运行期错误的码，
     * 而 9 只可能来自自测，便于判据区分「切换坏了」与「程序自己坏了」）。 */
    if (g11_selftest != 0 && rt_switch_selftest() != 0) {
        rt_exit(9);
    }

    /* 调度骨架自测（`GO-2`）：建两个 G 各打印一句、`main` 也入队，期望
     * 输出恰好 `A\nB\n`。 */
    if (g11_schedtest != 0) {
        rt_schedtest();
    }

    /* `rt_go` 自测（Task 12）：手工启动一个 G，期望输出 `A\n`。 */
    if (g11_gotest != 0) {
        rt_gotest();
    }

    /* chan 直通自测（Task 17）：cap=3 发 3 收 3 + closed 后零值。 */
    if (g11_chan2test != 0) {
        rt_chan2test();
    }

    /* 阻塞 + 唤醒自测（Task 18）：无缓冲，sender G 发、main 收。 */
    if (g11_chan3test != 0) {
        rt_chan3test();
    }
}

/* mmap 一个新 chunk 挂到链头。`need` 是「至少要能装下的 data 字节」。
 *
 * # 三条与 M9 不同的地方（`C12` §1.1）
 *
 * ① **16 MiB 对齐**：`mmap(size + 16 MiB)` 再把前缀 `munmap` 掉。这样
 *    `p >> 24` 就是 chunk id，`mark_ptr` 判「在堆里吗」是 O(1)。
 * ② **块头 64 字节在数据区之内**，`bump` 从 64 起 —— 所以 `base` 到
 *    `base+64` 不是块，`chunk_of` 要排除这一段。
 * ③ **旁挂两张元数据表**（起始位图 + 页摘要），一次 mmap 两块。
 */
static u64 rt_align_up(u64 v, u64 a) { return (v + a - 1) & ~(a - 1); }

void rt_grow(u64 need) {
    /* 单段 16 MiB 装不下 ⇒ 按 16 MiB 的倍数扩（多段各登记一次 id）。 */
    u64 size = CHUNK_MIN;
    if (need + CHUNK_HDR > size) {
        size = rt_align_up(need + BLK_HDR, CHUNK_MIN);
    }
    u64 bm_size = size / 64;    /* 1 位 / 8 字节 */
    u64 sum_size = size / 32768; /* 1 位 / 4 KiB */
    u64 total = size + bm_size + sum_size;
    if (heap_committed + total > heap_limit) {
        rt_fatal_oom(heap_limit);
    }

    /* 主区：多要一个 chunk 的余量用于对齐，前缀与后缀都还掉。 */
    i64 r = sys6(SYS_mmap, 0, (i64)(size + CHUNK_MIN), PROT_READ_WRITE, MAP_PRIVATE_ANON, -1, 0);
    if (r < 0) {
        rt_fatal_oom(heap_limit);
    }
    u64 base = rt_align_up((u64)r, CHUNK_MIN);
    if (base > (u64)r) {
        sys3(SYS_munmap, r, (i64)(base - (u64)r), 0);
    }
    /* 后缀：`base + size` 到 `r + size + 16 MiB` 都是多余的虚拟空间。 */
    u64 tail = (u64)r + size + CHUNK_MIN - (base + size);
    if (tail > 0) {
        sys3(SYS_munmap, (i64)(base + size), (i64)tail, 0);
    }

    /* 元数据：位图在前、页摘要在后（一次 mmap，两段紧邻）。 */
    i64 m = sys6(SYS_mmap, 0, (i64)(bm_size + sum_size), PROT_READ_WRITE, MAP_PRIVATE_ANON, -1, 0);
    if (m < 0) {
        sys3(SYS_munmap, (i64)base, (i64)size, 0);
        rt_fatal_oom(heap_limit);
    }

    Chunk *c = (Chunk *)base;
    c->next = heap;
    c->base = base;
    c->size = size;
    c->bump = CHUNK_HDR;
    c->free_head = 0;
    c->meta_bm = (u8 *)m;
    c->meta_sum = (u8 *)m + bm_size;
    c->live_bytes = 0;
    heap = c;
    heap_committed += total;

    /* 每段一个 id（`size` 是 16 MiB 的倍数 ⇒ 段数 = size >> 24）。 */
    for (u64 id = base >> CHUNK_SHIFT; id <= (base + size - 1) >> CHUNK_SHIFT; id++) {
        chunk_tab_put(id, c);
    }
}

/* ── 起始位图与页摘要（`C12` §3）── */

/* 位下标一律用**相对 base 的字节偏移**：`off` 的位在 `bm[off >> 6]` 的
 * `(off >> 3) & 7` 位，覆盖 `[base+off, base+off+8)`。 */
static void bm_set(Chunk *c, u64 off) { c->meta_bm[off >> 6] |= (u8)(1u << ((off >> 3) & 7)); }
static void bm_clear(Chunk *c, u64 off) { c->meta_bm[off >> 6] &= (u8)~(1u << ((off >> 3) & 7)); }
static int bm_test(Chunk *c, u64 off) { return (c->meta_bm[off >> 6] >> ((off >> 3) & 7)) & 1; }

static void sum_set(Chunk *c, u64 page) { c->meta_sum[page >> 3] |= (u8)(1u << (page & 7)); }
static void sum_clear(Chunk *c, u64 page) { c->meta_sum[page >> 3] &= (u8)~(1u << (page & 7)); }
static int sum_test(Chunk *c, u64 page) { return (c->meta_sum[page >> 3] >> (page & 7)) & 1; }

/* 从一个**可能是内部**的地址回到块首（`C12` §3.3 / §3.4）。
 *
 * # 为什么不能只看「本页有没有起点」
 *
 * 本页**有**起点但都 `> p` 时，`p` 的块起点在更早的页（一个 1 MiB 数组
 * 的第 3 个字节：起点在页首，而本页还有**下一个块**的起点 > p）。
 * 漏了这条退化，症状是「`&s[3]` 返回 null，而 `&s[len-1]` 碰巧对」。
 */
static u8 *resolve_block(Chunk *c, u64 a) {
    if (a < c->base + CHUNK_HDR || a >= c->base + c->size) {
        return 0;
    }
    u64 off = a - c->base;
    u64 page = off >> 12;

    /* ① 本页内向下扫（从 p 所在的字起）。 */
    if (sum_test(c, page)) {
        u64 w = off & ~7UL;
        u64 lo = page << 12;
        while (w >= lo) {
            if (bm_test(c, w)) {
                return (u8 *)(c->base + w);
            }
            if (w < 8) {
                break;
            }
            w -= 8;
        }
    }
    /* ② 退化：往前找最近的有起点的页，从该页最后一个字向下扫。 */
    for (u64 q = page; q > 0; q--) {
        u64 pq = q - 1;
        if (!sum_test(c, pq)) {
            continue;
        }
        u64 w = ((pq + 1) << 12) - 8;
        u64 lo = pq << 12;
        while (w >= lo) {
            if (bm_test(c, w)) {
                return (u8 *)(c->base + w);
            }
            if (w < 8) {
                break;
            }
            w -= 8;
        }
    }
    return 0;
}

/* ── 托管分配（`C12` §1.3 / §2）── */

/* 从当前 bump 切一块 `blk` 字节，置起始位与页摘要，返回块首。 */
static u8 *bump_take(Chunk *c, u64 blk) {
    u64 off = c->bump;
    c->bump = off + blk;
    bm_set(c, off);
    sum_set(c, off >> 12);
    return (u8 *)(c->base + off);
}

/* ── 块大小与 free list（`C12` §2.1 / §1.3）──
 *
 * # 为什么块大小必须能从**块头**读出来
 *
 * 一开始想按 `kind + n + desc` 算（`object` 用 `desc->size`、`array` 用
 * `elem->size * n` …）。但那样 `desc` 非法（`bad_desc`）时就算不出大小 ——
 * 而 sweep 是按地址顺序走的，算错一步就会**走到块中间**、把中间当块首，
 * 之后整条链都错位。契约里说「宁可泄漏也不误扫」，所以大小必须是**自证的**。
 *
 * 于是 `+8` 的高 29 位放 `blk >> 4`（块大小恒 16 的倍数，最大 16 MiB ⇒
 * 只需 20 位）。代价是 `n` 与 `kind` 各让出一点位宽，都够用。
 */
#define NK_N(nk) ((nk) & 0xFFFFFFFFUL)
#define NK_KIND(nk) (((nk) >> 32) & 7UL)
#define NK_BLK(nk) ((((nk) >> 35) & 0x1FFFFFFFUL) << 4)
#define NK_MAKE(n, kind, blk) ((n) | ((u64)(kind) << 32) | (((blk) >> 4) << 35))

/* 块的总字节数（活块看 `+8`，空闲块看 `header` 的 bit32..63）。 */
static u64 blk_size_of(u64 h, u64 nk) {
    if (h & HDR_FREE) {
        return h >> 32;
    }
    return NK_BLK(nk);
}

/* 把一个 `[off, off+len)` 区间做成空闲块并链进本 chunk 的 free list。
 *
 * 空闲块的形状与活块**不同**：`header = HDR_FREE | (len << 32)`、`+8 = next`
 * （相对 base 的偏移，0 = 链尾）。所以任何读块头的地方都要先看 bit0。 */
static void free_push(Chunk *c, u64 off, u64 len) {
    u8 *blk = (u8 *)(c->base + off);
    *(u64 *)blk = HDR_FREE | (len << 32);
    *(u64 *)(blk + 8) = c->free_head;
    c->free_head = off;
    /* ⚠️ **空闲块的起点也要置位**（`R-10-20`）。起始位图的语义是
     * 「这里有一个块头」，**不区分活块与空闲块**。漏了它，`resolve_block`
     * 对「指向空闲区的指针」会往前找到**残留的旧位**（某个早先被回收的块），
     * 把空闲区里的旧字节当成块头读 —— `h` 是旧的 desc 指针（bit0 是 0），
     * 于是被当成**活块**标记、进 worklist、`scan_block` 拿旧 desc 去扫
     * 垃圾指针 ⇒ 段错误。实测 `ken/simpswitch.go` 正是这样崩的。 */
    bm_set(c, off);
    sum_set(c, off >> 12);
}

/* first-fit 取一块，`*blk` 是**请求**大小、返回时是**实际**大小。
 *
 * 余量 >= 32 才切分：切出比 `BLK_MIN` 更小的尾巴等于制造一块永远用不上的
 * 碎片（它连一个块头都装不下）。 */
static u8 *free_take(Chunk *c, u64 *blk) {
    u64 *prev = &c->free_head;
    while (*prev != 0) {
        u64 off = *prev;
        u8 *b = (u8 *)(c->base + off);
        u64 len = *(u64 *)b >> 32;
        if (len >= *blk) {
            u64 rest = len - *blk;
            if (rest >= BLK_MIN + BLK_HDR) {
                u64 noff = off + *blk;
                u8 *nb = (u8 *)(c->base + noff);
                *(u64 *)nb = HDR_FREE | (rest << 32);
                *(u64 *)(nb + 8) = *(u64 *)(b + 8);
                *prev = noff;
                bm_set(c, noff);
                sum_set(c, noff >> 12);
            } else {
                *prev = *(u64 *)(b + 8);
                *blk = len;
            }
            bm_set(c, off);
            sum_set(c, off >> 12);
            return b;
        }
        prev = (u64 *)(b + 8);
    }
    return 0;
}

/* `rt_gc` 定义在文件后半（要用 `TypeDesc`），这里先声明给 `gc_alloc` 的
 * stress 钩子用。 */
void rt_gc(void);

/* 自动触发（`C12` §5.3，`T-GC-01`）。
 *
 * # 策略：**上次存活量 × 2**，但不低于 `gc_threshold`
 *
 * ```text
 * next = max(gc_threshold, gc_live_bytes * 2)
 * gc_since >= next  ⇒  rt_gc()
 * ```
 *
 * # 为什么不是「分配失败才收」
 *
 * 计划初稿写的是「分配失败且 `heap_committed > threshold` ⇒ 收一次再重试」。
 * 那条**永远不会触发**：chunk 是 16 MiB，而语料里的程序分配量都在 16 MiB
 * 以内 —— bump 永远够，于是「自动触发」形同虚设，`on` 态退化成 `off` 态。
 * `T-GC-01`（`RGOC_GC_THRESHOLD=65536` + 1 MiB 分配 ⇒ `collections >= 1`）
 * 就是为了把这条钉住。
 *
 * # 为什么看 `gc_since` 而不是 `heap_committed`
 *
 * `heap_committed` 是 **mmap 量**，一跳就是 16 MiB，粒度太粗；`gc_since` 是
 * 真实分配量，能反映「垃圾涨得多快」。
 *
 * `gc_live_bytes` 由 `sweep` 在每次 GC 末尾写（上次清扫后的存活字节）——
 * 于是阈值随存活集自适应：堆越大，收得越少（与 Go 的 `GOGC=100` 同形）。
 */
static void gc_maybe(void) {
    if (gc_pause != 0 || gc_on == 0) {
        return;
    }
    u64 next = gc_live_bytes * 2;
    if (next < gc_threshold) {
        next = gc_threshold;
    }
    if (gc_since >= next) {
        rt_gc();
    }
}

/* 核心分配：`need` 是 data 字节数。
 *
 * # 为什么 `need_zero` 不在参数里
 *
 * Go 的零值语义要求 `new(T)` 一定是零 —— 而**复用的块**里躺着上一条命的
 * 数据。所以清零不是可选项，是必做项（`C12` §1.3）。`rt_alloc` 保留
 * `need_zero` 形参只为 ABI 稳定（`C5` §11.1），实现里恒清零。
 *
 * # `K_ARENA` 为什么也走这里
 *
 * arena 的结构（itab / DeferRec / 输出缓冲）与 Go 对象**同一个 chunk 链**
 * （省一套记账），但 kind = `K_ARENA` ⇒ 清扫跳过、扫描跳过。若不给它们
 * 块头，清扫按位图走时会把它们当成「空闲」并塞进 free list —— 之后被
 * `gc_alloc` 复用就覆盖了 itab。
 */
static void *gc_alloc(u64 need, u64 kind, const void *desc, u64 n) {
    u64 blk = rt_align_up(BLK_HDR + need, 16);
    if (blk < BLK_MIN) {
        blk = BLK_MIN;
    }

    gc_since += need;
    gc_maybe();

    /* stress：每 `gc_stress` 次分配先收一次（`C12` §5.1 / `R-10-10`）。
     *
     * ⚠️ `gc_stress` 是**间隔**不是开关：`=1` 是「每次分配都收」，代价
     * O(n²) —— `gc1.go` 的 1e5 次分配 × 平均 17 万块 = 1.7e10 次块扫描，
     * 语料里跑不完。所以小 fixture 用 `=1`（最强证据），语料用 `=64`。 */
    if (gc_pause == 0 && gc_stress != 0 && gc_on != 0 && (gc_allocs % gc_stress) == 0) {
        rt_gc();
    }

    /* 顺序是 **free list → bump → 新 chunk**（`C12` §1.3）。
     *
     * # ⚠️ 为什么 free list 必须在 bump 之前
     *
     * 第一版写成「先 bump、bump 用完再用 free list」，实测抓到一个**判据假绿**：
     * `G` 的块被回收进了 free list，但 churn 的分配一直走 bump（16 MiB 用完
     * 就开新 chunk），于是那个块的内容**从没被覆盖** —— `G[0]` 照样读到原值，
     * 「全局根保活」的变异测试是绿的。
     *
     * 语义上也该如此：free list 里的块是**已经付过 mmap 代价**的，优先复用
     * 它们才是「堆不增长」；先 bump 会把每个 chunk 都撑满才回头复用，
     * 白白多占几个 chunk。 */
    u8 *p = 0;
    for (Chunk *c = heap; c != 0 && p == 0; c = c->next) {
        p = free_take(c, &blk);
    }
    if (p == 0) {
        for (Chunk *c = heap; c != 0; c = c->next) {
            if (c->bump + blk <= c->size) {
                p = bump_take(c, blk);
                break;
            }
        }
    }
    if (p == 0) {
        rt_grow(blk);
        p = bump_take(heap, blk);
    }

    /* 块头两行（`C12` §2.1）。`desc` 8 对齐 ⇒ 低 3 位本来就是 0。 */
    u64 h = (u64)desc;
    if (kind == K_RAW || kind == K_ARENA) {
        h |= HDR_RAW;
    }
    *(u64 *)p = h;
    *(u64 *)(p + 8) = NK_MAKE(n, kind, blk);

    /* 清零整块 data（含尾部 padding）：复用的块里躺着上一条命的数据。 */
    rt_memset(p + BLK_HDR, 0, blk - BLK_HDR);

    gc_allocs++;
    if (kind < 5) {
        gc_kind_count[kind]++;
    }
    gc_alloc_bytes += need;
    return p + BLK_HDR;
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

/* arena 分配（`C12` §6）：**永不回收、不扫描**的运行时结构。
 *
 * # M10 起它也带 16 字节块头
 *
 * 原因不是「想要头」，而是**清扫必须能跳过它**：sweep 按起始位图走，
 * 位图没标记的地方就是空闲 —— 而 arena 的结构如果不在位图里，就会被当成
 * 空闲块塞进 free list，之后被 `gc_alloc` 复用就覆盖了 itab / DeferRec。
 * 给了头 + `K_ARENA` 之后，sweep 见到 kind==5 直接跳过。
 *
 * # `align` 参数为什么不用
 *
 * 块大小恒 16 的倍数、`base` 16 MiB 对齐 ⇒ `data` 恒 16 对齐，任何
 * `align <= 16` 都满足。保留形参只为不改调用点（`C12` §6 的守卫按调用点
 * 扫源码，改签名会连带改那张表）。
 */
void *rt_arena_alloc(u64 size, u64 align) {
    (void)align;
    if (size == 0) {
        return rt_zerobase;
    }
    return gc_alloc(size, K_ARENA, 0, size);
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

/* ════════════════════════════════════════════════════════════════════
 * `hchan`（M11b/Task 16，`C11` §7 **布局冻结** / §3.2 四张表）
 * ════════════════════════════════════════════════════════════════════
 *
 * 字段序**逐字照抄** `rgoc-arm64::hchan::Hchan`（那是 `t_sch_01` 对账的对象）：
 *
 * ```text
 *   +0  qcount     len(c)
 *   +8  dataqsiz   cap(c)
 *   +16 buf        环形缓冲（托管堆；dataqsiz == 0 时为 0）
 *   +24 elemsize   元素宽度（u16）
 *   +26 (padding)  u32 的 4 字节对齐
 *   +28 closed     单向 0 → 1
 *   +32 elemtype   元素 desc（GC 扫缓冲元素时要它）
 *   +40 sendx      环形发送下标
 *   +48 recvx      环形接收下标
 *   +56 recvq      Waitq { head, tail }
 *   +72 sendq      Waitq { head, tail }
 *   +88 lock       留位给 M11e（M11b 是 M:1，不用）
 *   = 96 字节
 * ```
 *
 * ⚠️ **`closed` 在 +28 而不是 +26**：`elemsize` 是 `u16`，而 `u32` 要 4 字节
 * 对齐 ⇒ 中间有 2 字节 padding。`_Static_assert` 钉住这一点 —— 写错的话
 * `closed` 会与 `elemsize` 的高 16 位重叠，症状是「关闭标记被元素宽度污染」。
 */
typedef struct Sudog Sudog;

typedef struct Hchan {
    u64 qcount;
    u64 dataqsiz;
    u8 *buf;
    u16 elemsize;
    u32 closed;
    const void *elemtype;
    u64 sendx;
    u64 recvx;
    Sudog *recvq_head;
    Sudog *recvq_tail;
    Sudog *sendq_head;
    Sudog *sendq_tail;
    u64 lock;
} Hchan;

_Static_assert(sizeof(Hchan) == 96, "Hchan 必须是 96 字节（C11 §7）");
_Static_assert(__builtin_offsetof(Hchan, dataqsiz) == 8, "dataqsiz 在 +8");
_Static_assert(__builtin_offsetof(Hchan, buf) == 16, "buf 在 +16");
_Static_assert(__builtin_offsetof(Hchan, elemsize) == 24, "elemsize 在 +24");
_Static_assert(__builtin_offsetof(Hchan, closed) == 28, "closed 在 +28（不是 +26）");
_Static_assert(__builtin_offsetof(Hchan, elemtype) == 32, "elemtype 在 +32");
_Static_assert(__builtin_offsetof(Hchan, sendx) == 40, "sendx 在 +40");
_Static_assert(__builtin_offsetof(Hchan, recvx) == 48, "recvx 在 +48");
_Static_assert(__builtin_offsetof(Hchan, recvq_head) == 56, "recvq 在 +56");
_Static_assert(__builtin_offsetof(Hchan, sendq_head) == 72, "sendq 在 +72");
_Static_assert(__builtin_offsetof(Hchan, lock) == 88, "lock 在 +88");

/* `make(chan T[, n])`。
 *
 * ⚠️ **`gc_pause` 包住整段**（与 `rt_defer_push` 同源，`R-10-24`）：`Hchan`
 * 与 `buf` 都是托管堆块，而 stress 钩子在**分配之前**跑 GC —— 此刻它们还没
 * 发布到任何可达位置。
 *
 * ⚠️ **已知缺口（进 `M11-report.md`）**：`Hchan` 是 `K_RAW`，GC **不扫它的
 * 内容** ⇒ `buf` 那个指针没有根（`elemtype` 在 rodata，不需要）。M11b 的
 * a/b 阶段强制 `mode=off`（`D-M11-12`）所以碰不到；M11c 要么把 `Hchan` 换成
 * 带 desc 的类型，要么在 `mark_roots` 里扫一张 chan 链表。 */
/* 两个分配入口定义在文件后半（ / ）。 */
void *rt_alloc_raw(u64 bytes);
void *rt_alloc_array(const TypeDesc *elem, u64 cap);

void rt_makechan(Hchan **out, u64 dataqsiz, const void *elemtype) {
    /* `elemsize` 从 desc 的 `size` 读（`TypeDesc.size` 在 **+0**，`rgoc-meta`
     * 的 `desc.rs`）。**不由 isel 传**：`TypeTable` 在 arm64 侧没有「类型大小」
     * 的 API（大小是 `rgoc-meta` 算的），传两份口径迟早不一致。
     *
     * ⚠️ `TypeDesc.size` 是 **data 部分**（不含 8 字节对象头）—— 正是通道缓冲
     * 需要的元素宽度。 */
    u64 elemsize = elemtype != 0 ? (u64)(*(const u32 *)elemtype) : 0;
    gc_pause++;
    Hchan *c = (Hchan *)rt_alloc_raw(sizeof(Hchan));
    c->qcount = 0;
    c->dataqsiz = dataqsiz;
    c->elemsize = (u16)elemsize;
    c->closed = 0;
    c->elemtype = elemtype;
    c->sendx = 0;
    c->recvx = 0;
    c->recvq_head = 0;
    c->recvq_tail = 0;
    c->sendq_head = 0;
    c->sendq_tail = 0;
    c->lock = 0;
    if (dataqsiz > 0 && elemsize > 0) {
        c->buf = (u8 *)rt_alloc_array((const TypeDesc *)elemtype, dataqsiz);
    } else {
        /* `chan [0]byte` / `chan struct{}`：`elemsize == 0` ⇒ 不要缓冲，
         * 但 `dataqsiz` 可以非 0（`zerosize.go` 就是这么用的）。 */
        c->buf = 0;
    }
    gc_pause--;
    /* 输出参数而不是返回值：与 `rt_makeslice` 同一形状（`x0` 是目标槽地址）。
     * 槽在栈上 ⇒ GC 顺着帧记录扫得到它。 */
    *out = c;
}

/* ── hchan 的四操作（M11b/Task 17–19，`C11` §3.2 四张表）───────────────
 *
 * Task 17 只做**不阻塞**的两条：缓冲未满的 `send`、缓冲非空的 `recv`。
 * 阻塞（`recvq` / `sendq` / `Sudog`）是 Task 18。
 */

void panic_set(const char *m, u64 n); /* 定义在文件后半 */

/* `panic: send on closed channel` —— `C11` §5 的文本，**不带** `runtime error:`。 */
static void rt_panic_send_closed(void) {
    const char *m = "panic: send on closed channel";
    u64 n = 0;
    while (m[n] != 0) {
        n++;
    }
    panic_set(m, n);
}

/* Task 18 之前：阻塞路径直接退出 9。
 *
 * 用 **9** 而不是 2：`2` 是「跑错了」（`C9` §5），而这是「还没实现」。
 * 判据据此能分辨「实现有 bug」与「这一块还没做」。 */
static void rt_panic_blocked(const char *what) {
    rt_write(2, "rgoc: ", 6);
    u64 k = 0;
    while (what[k] != 0) {
        k++;
    }
    rt_write(2, what, k);
    rt_write(2, "\n", 1);
    rt_exit(9);
}

/* `ch <- v`。
 *
 * ⚠️ **`closed` 检查排第一**（`C11` §3.2 的 ⚠️）。顺序反了会让「往已关闭的
 * 有缓冲通道发送」**静默塞进缓冲**，而不是 panic —— 那是最难查的一类语义漂移，
 * 变异 `M-M11-07` 就是它。 */
/* ── `Sudog` 与阻塞（M11b/Task 18，`C11` §7 / §3.2）───────────────────
 *
 * 字段序照抄 `rgoc-arm64::hchan::Sudog`（`#[repr(C)]`，48 字节）。
 *
 * ⚠️ **与设计的偏差（记进 `M11-report.md`）**：`C11` §7 的 `T-CHAN-08` 说
 * `Sudog` 放**托管堆**。这里放在**当前 G 的栈上**（局部变量）—— 理由：G 在
 * 阻塞期间栈一定活着（它只是 `state = blocked`，没有被回收），而栈上的
 * `Sudog` 让「登记 → 阻塞 → 醒来」的生存期由 C 的作用域自动保证。
 * 等 M11c 有了 G 的回收再改回托管堆（那时栈可能被收）。
 */
typedef struct Sudog {
    GDesc *g;            /* +0  等着的 G */
    void *elem;          /* +8  值的位置（发送方：值在这；接收方：对端写到这）*/
    struct Sudog *next;  /* +16 */
    struct Sudog *prev;  /* +24 撤销要 O(1) 摘链 */
    u64 token;           /* +32 select 的撤销句柄（M11b 后半）*/
    u32 done;            /* +40 已被对端完成（`C11` §3.1 的唯一判据）*/
} Sudog;

_Static_assert(sizeof(Sudog) == 48, "Sudog 必须是 48 字节（C11 §7）");
_Static_assert(__builtin_offsetof(Sudog, elem) == 8, "elem 在 +8");
_Static_assert(__builtin_offsetof(Sudog, next) == 16, "next 在 +16");
_Static_assert(__builtin_offsetof(Sudog, prev) == 24, "prev 在 +24");
_Static_assert(__builtin_offsetof(Sudog, token) == 32, "token 在 +32");
_Static_assert(__builtin_offsetof(Sudog, done) == 40, "done 在 +40");

static void waitq_push(Sudog **head, Sudog **tail, Sudog *s) {
    s->next = 0;
    s->prev = *tail;
    if (*tail != 0) {
        (*tail)->next = s;
    } else {
        *head = s;
    }
    *tail = s;
}

static void waitq_remove(Sudog **head, Sudog **tail, Sudog *s) {
    if (s->prev != 0) {
        s->prev->next = s->next;
    } else {
        *head = s->next;
    }
    if (s->next != 0) {
        s->next->prev = s->prev;
    } else {
        *tail = s->prev;
    }
    s->next = 0;
    s->prev = 0;
}

static Sudog *waitq_pop(Sudog **head, Sudog **tail) {
    Sudog *s = *head;
    if (s != 0) {
        waitq_remove(head, tail, s);
    }
    return s;
}

/* 唤醒：`done = 1` + 状态 ready + **入队尾**（`C11` §3.2 的「唤醒」那行）。 */
static void rt_wake(Sudog *s) {
    s->done = 1;
    if (s->g != 0) {
        s->g->state = 0; /* ready */
        rt_enqueue(s->g);
    }
}

/* `ch <- v`。
 *
 * `C11` §3.2 的三步，**顺序不能改**：
 * ① `closed` 检查 —— 排第一（顺序反了会让「往已关闭的有缓冲通道发送」静默
 *    塞进缓冲而不是 panic，变异 `M-M11-07`）；
 * ② 有接收方在等 ⇒ **直给**（不经过 buf）；
 * ③ 缓冲未满 ⇒ 塞 buf；
 * ④ 否则阻塞。
 */
void rt_send(Hchan *c, const void *val) {
    if (c->closed != 0) {
        rt_panic_send_closed();
        return;
    }
    u64 es = c->elemsize;
    Sudog *r = waitq_pop(&c->recvq_head, &c->recvq_tail);
    if (r != 0) {
        if (es > 0 && r->elem != 0) {
            rt_memcpy(r->elem, val, es);
        }
        rt_wake(r);
        return;
    }
    if (c->dataqsiz > 0 && c->qcount < c->dataqsiz) {
        if (es > 0) {
            rt_memcpy(c->buf + c->sendx * es, val, es);
        }
        c->sendx = (c->sendx + 1) % c->dataqsiz;
        c->qcount++;
        return;
    }
    /* ④ 阻塞：登记到 `sendq` 尾 ⇒ 换 G ⇒ 醒来时值已被接收方取走 */
    Sudog s;
    s.g = g_cur;
    s.elem = (void *)val;
    s.next = 0;
    s.prev = 0;
    s.token = 0;
    s.done = 0;
    waitq_push(&c->sendq_head, &c->sendq_tail, &s);
    g_cur->state = 2; /* blocked */
    rt_schedule();
}

/* `<-ch`。与 `rt_send` 对称，另外多一条 `closed && 空 ⇒ 零值`。 */
void rt_recv(Hchan *c, void *out) {
    u64 es = c->elemsize;
    /* ① 有发送方在等 ⇒ 直取 */
    Sudog *w = waitq_pop(&c->sendq_head, &c->sendq_tail);
    if (w != 0) {
        if (es > 0) {
            if (w->elem != 0) {
                rt_memcpy(out, w->elem, es);
            } else {
                rt_memset(out, 0, es);
            }
        }
        rt_wake(w);
        return;
    }
    /* ② 缓冲非空 ⇒ 取 */
    if (c->qcount > 0) {
        if (es > 0) {
            rt_memcpy(out, c->buf + c->recvx * es, es);
        }
        c->recvx = (c->recvx + 1) % c->dataqsiz;
        c->qcount--;
        return;
    }
    /* ③ 已关闭且空 ⇒ 零值 */
    if (c->closed != 0) {
        if (es > 0) {
            rt_memset(out, 0, es);
        }
        return;
    }
    /* ④ 阻塞 */
    Sudog s;
    s.g = g_cur;
    s.elem = out;
    s.next = 0;
    s.prev = 0;
    s.token = 0;
    s.done = 0;
    waitq_push(&c->recvq_head, &c->recvq_tail, &s);
    g_cur->state = 2; /* blocked */
    rt_schedule();
}

/* `rt_send` / `rt_recv` 的自测（Task 17；`go` 语句的下降还没接 isel）。
 *
 * `cap = 3` 发 3 收 3，断言 FIFO 顺序；再置 `closed` 断言 recv 拿到零值。
 * 失败退出 **8**（与 9「未实现」、2「跑错了」都分开）。 */
void rt_chan2test(void) {
    /* 一个最小的 int desc：`size = 8`（+0），其余全 0 ⇒ 不扫指针。 */
    static const u64 fake_desc[5] = {8, 0, 0, 0, 0};
    Hchan *c = 0;
    rt_makechan(&c, 3, fake_desc);
    u64 v = 11;
    rt_send(c, &v);
    v = 22;
    rt_send(c, &v);
    v = 33;
    rt_send(c, &v);
    u64 o = 0;
    rt_recv(c, &o);
    if (o != 11) {
        rt_exit(8);
    }
    rt_recv(c, &o);
    if (o != 22) {
        rt_exit(8);
    }
    rt_recv(c, &o);
    if (o != 33) {
        rt_exit(8);
    }
    /* 关闭后 recv ⇒ 零值 */
    c->closed = 1;
    o = 99;
    rt_recv(c, &o);
    if (o != 0) {
        rt_exit(8);
    }
}

/* `rt_send` / `rt_recv` 的**阻塞 + 唤醒**自测（Task 18）。
 *
 * 无缓冲通道：sender G 发 42、main 收。两边都会阻塞一次 ——
 * ① main 先 `rt_recv`（此刻 sender 只入队还没跑）⇒ recvq 空 ⇒ main 阻塞；
 * ② 调度到 sender ⇒ `rt_send` 看到 recvq 里有 main ⇒ 拷值 + 唤醒；
 * ③ sender `rt_goexit` ⇒ 切回 main ⇒ `rt_recv` 返回 42。
 *
 * 这条**同时**验了三件事：Sudog 登记、`g_cur->state = blocked` + 换 G、
 * 对端唤醒后从 `rt_schedule` 正确返回。任一处错都会挂死或拿到错值。
 */
static Hchan *sc_ch3;
static u64 sc_v3;

static void sc_sender3(void) {
    rt_send(sc_ch3, &sc_v3);
    rt_goexit();
}

void rt_chan3test(void) {
    static const u64 fake_desc[5] = {8, 0, 0, 0, 0};
    sc_ch3 = 0;
    rt_makechan(&sc_ch3, 0, fake_desc); /* 无缓冲 */
    sc_v3 = 42;
    rt_go((void *)sc_sender3, 0, 0);
    u64 o = 0;
    rt_recv(sc_ch3, &o);
    if (o != 42) {
        rt_exit(8);
    }
}



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

/* 统一分配入口：`[+0 header][+8 n|kind][+16 data...]`（`C5` §11.1 / `C12` §2）。
 *
 * # `need_zero` 现在是**无用形参**
 *
 * M9 的注释预言过「M10 开始复用对象时它是唯一防线」—— 而现在它连防线都
 * 不是了：`gc_alloc` **恒清零** data（复用的块里躺着上一条命的数据，
 * 而 Go 的零值语义要求 `new(T)` 一定是零）。所以 `need_zero` 保留只为
 * ABI 稳定（`isel` 照旧按 `needs_zero(类型)` 传，`mem_alloc.rs` 的实参判据
 * 仍然成立），实现里不看它。记 `R-10-5`。
 */
void *rt_alloc(const TypeDesc *desc, i64 need_zero) {
    (void)need_zero;
    u64 size = desc->size;
    if (size == 0) {
        /* 零大小对象：所有实例共享 `rt_zerobase`（`zerosize.go` 要求
         * `&x == new([0]int)`）。它没有可写的 data，也不写 desc ——
         * 那块内存是**所有**零大小类型共用的，写谁的 desc 都是错的。 */
        return rt_zerobase;
    }
    u8 *p = (u8 *)gc_alloc(size, K_OBJECT, desc, 0);
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
    /* 返回 **data**（块首 + 16），不是块首：Go 侧的指针必须指到值上，
     * 否则字段偏移 0 会与对象头重叠（`M9` 的 `D-M9-1` 从没真正生效）。 */
    return p;
}

/* ── 三种托管分配入口（`C12` §2.2 / `D-M10-13`）──
 *
 * 每种都带自己的 kind，因为 GC 扫它们的方式不同：
 * `array` 按元素 desc 扫 n 个元素、`closure` 扫 n 个捕获字、`raw` 不扫。
 */

/* 数组：`elem` 是**元素** desc（`[]T` 的 `elem`），`cap` 是容量。
 *
 * 用 `cap` 而不是 `len` 作 `n`：`len` 之外的元素在 Go 里是「未初始化但可写」
 * 的槽，里面可能有上一条命的指针（复用块）。`gc_alloc` 已经清零了，所以
 * 按 `cap` 扫是**安全且完整**的；按 `len` 扫则会让 `s = s[:cap]` 之后的
 * 元素变成「GC 看不见的指针」。 */
void *rt_alloc_array(const TypeDesc *elem, u64 cap) {
    return gc_alloc((u64)elem->size * cap, K_ARRAY, elem, cap);
}

/* 裸字节：字符串本体 / `[]byte` 的底层存储。**不扫描**（里面没有指针）。 */
void *rt_alloc_raw(u64 bytes) { return gc_alloc(bytes, K_RAW, 0, bytes); }

/* 逃逸的取址槽（`R-10-23`）：`p := &x` 里 `p` 逃出函数时，`x` 的存储放这里。
 *
 * # 为什么必须进托管堆
 *
 * M9 用的是 `rt_arena_alloc`（arena **不扫描**）。于是「闭包里改了 `x`，
 * 而 `x` 是个指针」这种形状就丢了根：闭包捕获的是 `&槽`，`mark_ptr(&槽)` 在
 * 栈上 ⇒ `chunk_of` 返回 0 ⇒ 槽里那个指针一个字都没被标。
 *
 * 实测：`p := new([1024]byte); f := func() int { return p[0] }; churn(); f()`
 * 在 stress 下打印 204 而不是 42 —— 1 KiB 的块在 churn 里被回收了。
 *
 * # 为什么要 `gc_pause`
 *
 * 它在**序言**里分配，此时帧记录刚写好、`sp_idx = 0`；而块要等到 `st_x` 之后
 * 才可达。中间不能有 GC。关掉触发是安全的：从分配到发布之间没有别的分配点。 */
void *rt_alloc_escaped(const TypeDesc *desc) {
    gc_pause++;
    void *p = gc_alloc(desc->size, K_OBJECT, desc, 0);
    gc_pause--;
    return p;
}

/* 闭包：`{fn, cap0, cap1…}`。返回 data（`fn` 已写好），isel 只负责填捕获。 */
void *rt_alloc_closure(const void *fn, u64 ncaps) {
    void **p = gc_alloc(8 + 8 * ncaps, K_CLOSURE, 0, ncaps);
    p[0] = (void *)fn;
    return p;
}

/* ════════════════════════════════════════════════════════════════════
 * 标记 / 清扫 / `rt_gc`（M10/Task 11–13，`C12` §3.5–§3.6）
 * ════════════════════════════════════════════════════════════════════ */

/* 全局根表（由 `.s` 发）：`rt_globals[i] = {desc, addr}`。
 * 零大小全局**不在表里**（它们没有可扫的字，`module.rs::emit_global_roots`）。 */
typedef struct {
    const void *desc;
    u8 *addr;
} GlobalRoot;

extern GlobalRoot rt_globals[];
extern u64 rt_nglobals;

/* 标记栈（worklist）。
 *
 * # 为什么用 mmap 而不是 `gc_alloc`
 *
 * `H5`：**GC 期间不分配**。标记栈如果走 `gc_alloc`，它的 stress 钩子会在
 * 标记中途再触发一次 `rt_gc`（递归），而那次 GC 看到的是「标记了一半」的
 * 堆 —— 会把还没标记到的活对象当死的收掉。
 *
 * # 为什么不用递归
 *
 * 深链（1e5 个节点的链表）会爆栈，而 M10 的帧已经很紧。
 */
static void **gc_wl;
static u64 gc_wl_len;
static u64 gc_wl_cap;

static void wl_grow(void) {
    u64 ncap = gc_wl_cap == 0 ? 65536 : gc_wl_cap * 2;
    i64 r = sys6(SYS_mmap, 0, (i64)(ncap * 8), PROT_READ_WRITE, MAP_PRIVATE_ANON, -1, 0);
    if (r < 0) {
        rt_fatal_oom(heap_limit);
    }
    if (gc_wl != 0) {
        rt_memcpy((void *)r, gc_wl, gc_wl_len * 8);
        sys3(SYS_munmap, (i64)gc_wl, (i64)(gc_wl_cap * 8), 0);
    }
    gc_wl = (void **)r;
    gc_wl_cap = ncap;
}

static void wl_push(void *p) {
    if (p == 0) {
        return;
    }
    if (gc_wl_len == gc_wl_cap) {
        wl_grow();
    }
    gc_wl[gc_wl_len++] = p;
}

/* 标记一个**可能**是堆指针的值（`C12` §4.1）。
 *
 * 第一步永远是「在堆里吗」—— `chunk_of` 返回 0 就立刻返回。**这一步是全部
 * rodata / 栈地址 / 代码地址被忽略的唯一机制**：没有它，`desc` 指针会被当成
 * 对象、`mark_ptr` 会把 `.rodata` 里的字节当块头读。
 */
static void mark_ptr(void *p) {
    if (p == 0) {
        return;
    }
    Chunk *c = chunk_of(p);
    if (c == 0) {
        return;
    }
    u8 *blk = resolve_block(c, (u64)p);
    if (blk == 0) {
        /* 在 chunk 范围内却解析不到块首 ⇒ 起始位图漏置位（`C12` §3.4）。
         * 忽略它（宁可泄漏），但计数器非 0 时测试会红。 */
        gc_bad_ptr++;
        return;
    }
    u64 h = *(u64 *)blk;
    if (h & (HDR_FREE | HDR_MARK)) {
        /* 指向空闲块**不算错**（`R-10-21`）。
         *
         * 位图是 live-out 的**保守超集**（`live.rs`）：safepoint 上「本指令
         * 自己的 def」也在集合里，而 GC 在指令执行前跑 ⇒ 那个槽里还是上一条命
         * 的值。所以「扫到一个指向已回收块的指针」是**设计内的常态**。
         *
         * 实测：`ken/simpswitch.go` 的字符串累加循环稳定 `bad_ptr=4`，4 次
         * 全是这一类；程序输出与 `off` 态逐字节相同。
         *
         * 真正的错（`bad_ptr` 要抓的）只有两类：**解析不到块首**（起始位图漏
         * 置位）与**块头非法**（desc/大小坏了）—— 那两个都会让扫描器扫错东西，
         * 而这一类只是多看一眼已经死掉的块。 */
        return;
    }
    /* 第二道防线（`R-10-20`）：起始位图万一还有假阳性，这里把「块头明显
     * 不像块」的情况挡掉。`scan_block` 拿一个假 desc 去读 `ptrdata` 会直接
     * 段错误，而「跳过」最坏只是泄漏 —— 契约里说得很清楚：宁可泄漏也不误扫。 */
    {
        u64 nk = *(u64 *)(blk + 8);
        u64 sz = blk_size_of(h, nk);
        if (NK_KIND(nk) > K_ARENA || sz < BLK_MIN || (u64)(blk - c->base) + sz > c->bump) {
            gc_bad_ptr++;
            return;
        }
    }
    *(u64 *)blk = h | HDR_MARK;
    wl_push(blk);
}

/* 按 `gcdata` 扫一段：`data[0, ptrdata)` 里每 8 字节 1 位（bit i ↔ 第 i 个字）。
 *
 * ⚠️ `ptrdata > 0` 而 `gcdata == 0` 是**非法 desc**（`C12` §2.3）：这时按 raw
 * 处理（不扫）并记 `bad_desc`。宁可泄漏也不误扫 —— 误扫会把一个整数当地址。 */
static void mark_words(const u8 *data, const TypeDesc *d) {
    if (d == 0 || d->ptrdata == 0) {
        return;
    }
    if (d->gcdata == 0) {
        gc_bad_desc++;
        return;
    }
    for (u64 w = 0; w * 8 < (u64)d->ptrdata; w++) {
        if (d->gcdata[w >> 3] & (u8)(1u << (w & 7))) {
            void *q;
            rt_memcpy(&q, data + w * 8, 8);
            mark_ptr(q);
        }
    }
}

/* 扫一个块的内容，把里面的指针压进 worklist（`C12` §2.2 的五种 kind）。 */
static void scan_block(u8 *blk) {
    u64 h = *(u64 *)blk;
    u64 nk = *(u64 *)(blk + 8);
    u64 kind = NK_KIND(nk);
    u64 n = NK_N(nk);
    u8 *data = blk + BLK_HDR;
    if (kind == K_RAW || kind == K_ARENA) {
        return;
    }
    switch (kind) {
    case K_OBJECT:
        mark_words(data, (const TypeDesc *)HDR_DESC(h));
        break;
    case K_ARRAY: {
        const TypeDesc *e = (const TypeDesc *)HDR_DESC(h);
        if (e == 0 || e->ptrdata == 0) {
            if (e == 0) {
                gc_bad_desc++;
            }
            break;
        }
        u64 esz = (u64)e->size;
        for (u64 k = 0; k < n; k++) {
            mark_words(data + k * esz, e);
        }
        break;
    }
    case K_CLOSURE:
        /* 字 0 是代码地址（不扫），字 1..n 是捕获（全是指针）。 */
        for (u64 k = 1; k <= n; k++) {
            void *q;
            rt_memcpy(&q, data + k * 8, 8);
            mark_ptr(q);
        }
        break;
    case K_MAPSLOTS: {
        /* data = `[+0 kdesc][+8 vdesc][+16 槽…]`（`C12` §2.2）。
         *
         * ⚠️ **自带 desc 头**（`R-10-17`）：`MapObj` 里也有 kdesc/vdesc，但
         * 槽块一旦被单独分配，扫它的人手里只有块本身 —— 回头找 `MapObj`
         * 需要在块里存一个 `MapObj*`，那是多余的一跳（而且 `MapObj` 自己
         * 也在托管堆里，扫的时候还没轮到它）。 */
        const TypeDesc *kd = *(const TypeDesc *const *)data;
        const TypeDesc *vd = *(const TypeDesc *const *)(data + 8);
        if (kd == 0 || vd == 0) {
            gc_bad_desc++;
            break;
        }
        u64 ksz = align8((u64)kd->size);
        u64 stride = 8UL + ksz + align8((u64)vd->size);
        u8 *sl = data + 16;
        for (u64 i = 0; i < n; i++) {
            u8 *slot = sl + i * stride;
            if (slot[0] != MAP_LIVE) {
                continue;
            }
            mark_words(slot + 8, kd);
            mark_words(slot + 8 + ksz, vd);
        }
        break;
    }
    default:
        /* `K_ARENA` 走上面的早返回，其它 kind 不存在。 */
        break;
    }
}

static void drain(void) {
    while (gc_wl_len > 0) {
        u8 *blk = (u8 *)gc_wl[--gc_wl_len];
        scan_block(blk);
    }
}

/* ── 栈根：帧记录链（`C12` §4.2，M10/Task 22–23）── */

/* 帧记录链的链头。由 `isel` 的序言/尾声维护（`g_frames = fp` / `= prev`），
 * 这里只**读**。`.bss` 天然为零 ⇒ 空链。 */
void *g_frames;

/* `RtFuncInfo` 的 C 镜像（`C5` §11.4，M9 已发在 `.rodata`）。
 *
 * ⚠️ `sp_off[i]` 是第 `i` 个 safepoint 位图在 `sp_bits` 里的**起始字下标**
 * （`R-9-33`），不是位偏移。写成「位偏移再除 64」会让三个 safepoint 共用
 * 同一个 quad（实测 `.Lspoff0 = [0,1,2]` 正是字下标）。 */
typedef struct {
    u32 frame_size;
    u32 n_sp;
    const u32 *sp_off;
    const u64 *sp_bits;
} RtFuncInfo;

/* 扫一帧的位图。位序：bit `i` ↔ `fp - frame_size + 8i`（`C5` §8.4 冻结）。
 *
 * 返回上一帧的 `fp`（0 = 链尾）。
 *
 * ⚠️ **绝对不要在这里改 `g_frames`**（第一版就是那么写的，`g_frames = prev`）：
 * `g_frames` 是**链头**，扫完一次链它就变成 0 了 —— 之后每次 GC 都是空链。
 * 实测症状：第一次 GC 打了 2 个 `F`，之后 100 次一个都没有，`live` 恒 0，
 * 而 `collections` 正常增长（看起来「GC 在跑」，其实什么都没扫）。 */
static u8 *mark_frame(u8 *fp) {
    u64 prev = *(u64 *)(fp + REC_PREV);
    const RtFuncInfo *fi = *(const RtFuncInfo **)(fp + REC_FI);
    u64 idx = *(u64 *)(fp + REC_SP);
    if (fi != 0) {
        if (idx >= (u64)fi->n_sp) {
            /* `sp_idx` 越界 ⇒ **跳过这一帧**（漏标总比乱扫好），但计数器
             * 非 0 时测试会红。 */
            gc_bad_sp++;
        } else {
            const u64 *bits = fi->sp_bits + fi->sp_off[idx];
            u64 words = ((u64)fi->frame_size / 8 + 63) / 64;
            u8 *base = fp - fi->frame_size;
            for (u64 w = 0; w < words; w++) {
                u64 word = bits[w];
                while (word != 0) {
                    u64 b = (u64)__builtin_ctzll(word);
                    word &= word - 1;
                    u64 bit = w * 64 + b;
                    if (bit * 8 >= (u64)fi->frame_size) {
                        continue;
                    }
                    void *q;
                    rt_memcpy(&q, base + bit * 8, 8);
                    mark_ptr(q);
                }
            }
        }
    }
    return (u8 *)prev;
}

/* 走完整条帧链。链是**从新到旧**的（每帧的 `prev` 指向它的调用者）。
 *
 * 用**局部游标**遍历，`g_frames` 只读 —— 它是链头，写它等于把链剪断。 */
static void mark_frames(void) {
    u8 *fp = (u8 *)g_frames;
    while (fp != 0) {
        fp = mark_frame(fp);
    }
}

/* defer 链的根（`C12` §4.1 第 3 类）。实现放在 `DeferRec` 定义之后。 */
static void mark_defer(void);

/* ── pin 槽：已分配但**还没发布**的块（`R-10-18`）──
 *
 * # 为什么需要
 *
 * `gc_alloc` 在 stress 下会在**分配之前**先跑一次 GC。所以「刚分配出来、
 * 还没写进任何可达位置」的块是**看不见的** —— 而多块构造函数（map 的
 * `MapObj` + 槽块）必然有一个瞬间同时持有两块，其中前一块还没发布。
 *
 * 实测：`map_new` 先建槽块、再建 `MapObj`，第二次分配前的 GC 把槽块收掉，
 * 而 `MapObj` 恰好**复用了同一块**（free list 优先）⇒ `m->slots` 指向自己的
 * 中间 ⇒ `m[0]` 读出来是 0。
 *
 * 深度只需要 2，给 8 是留余量。溢出**不静默**：超了就 abort —— 静默丢一个
 * pin 的后果是「随机时刻丢对象」，比崩难查一百倍。
 */
static void *gc_pin[8];
static u64 gc_npin;

static void gc_pin_push(void *p) {
    if (gc_npin >= 8) {
        rt_fatal_oom(0);
    }
    gc_pin[gc_npin++] = p;
}

static void gc_pin_pop(void) {
    if (gc_npin > 0) {
        gc_pin[--gc_npin] = 0;
    }
}

/* 扫根（`C12` §4.1）：**全局** / **栈** / **defer 链**。
 *
 * 还差的三类在 Task 25–26 接（map 槽 / 闭包捕获 / 数组元素）—— 那三类是
 * **由被标记的块自己带出来的**（`scan_block` 按 kind 扫），不需要在这里列。 */
static void mark_roots(void) {
    mark_frames();
    mark_defer();
    for (u64 i = 0; i < gc_npin; i++) {
        mark_ptr(gc_pin[i]);
    }
    for (u64 i = 0; i < rt_nglobals; i++) {
        const TypeDesc *d = (const TypeDesc *)rt_globals[i].desc;
        u8 *a = rt_globals[i].addr;
        if (d == 0 || a == 0) {
            continue;
        }
        mark_words(a, d);
    }
}

/* 清扫（`C12` §3.5）。
 *
 * # 为什么按地址顺序走一遍就够了
 *
 * 死块在地址上是**连续**的（相邻的死块天然合并成 run），所以只要「碰到活块
 * 就把攒着的 run flush 进 free list」，合并就自动完成了 —— 不需要额外的
 * 合并算法。走错的唯一可能是**块大小算错**，而大小是从块头读的（自证）。
 */
static void sweep(void) {
    u64 total_live = 0;
    for (Chunk *c = heap; c != 0; c = c->next) {
        c->free_head = 0;
        u64 off = CHUNK_HDR;
        u64 run_off = 0;
        u64 run_len = 0;
        u64 live = 0;
        while (off < c->bump) {
            u8 *blk = (u8 *)(c->base + off);
            u64 h = *(u64 *)blk;
            u64 nk = *(u64 *)(blk + 8);
            u64 sz = blk_size_of(h, nk);
            if (sz < BLK_MIN || off + sz > c->bump) {
                /* 块头坏了：**停在这里**而不是继续走。继续走只会在错的
                 * 偏移上读到垃圾，把更多活块当死块收掉。 */
                gc_bad_desc++;
                break;
            }
            if (h & HDR_FREE) {
                if (run_len == 0) {
                    run_off = off;
                }
                run_len += sz;
                /* 已经空闲的块也要清位：它会并进 `run_off` 那个更大的 run，
                 * 自己不再是块首。留着它 = 给 `resolve_block` 一个假块首。 */
                bm_clear(c, off);
            } else if (h & HDR_MARK) {
                if (run_len > 0) {
                    free_push(c, run_off, run_len);
                    run_len = 0;
                }
                *(u64 *)blk = h & ~HDR_MARK;
                live += sz;
            } else if (NK_KIND(nk) == K_ARENA) {
                /* arena 块永不回收（`R-10-6`）。 */
                if (run_len > 0) {
                    free_push(c, run_off, run_len);
                    run_len = 0;
                }
                live += sz;
            } else {
                if (run_len == 0) {
                    run_off = off;
                }
                run_len += sz;
                bm_clear(c, off);
                gc_frees++;
            }
            off += sz;
        }
        if (run_len > 0) {
            free_push(c, run_off, run_len);
        }
        c->live_bytes = live;
        total_live += live;
    }
    gc_live_bytes = total_live;
}

/* 一次完整的 STW 收集。**只在分配点同步跑**（`F10`：单线程、无抢占）。
 *
 * `RGOC_GC=0` 时立即返回 —— 那是「与 M9 等价」的对照态（`T-CORP-05`）。 */
void rt_gc(void) {
    if (gc_on == 0) {
        return;
    }
    /* GC 期间不再触发（`H5`：标记与清扫里不许分配）。 */
    gc_pause++;
    gc_collections++;
    mark_roots();
    drain();
    sweep();
    gc_since = 0;
    gc_pause--;
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

/* 槽状态在文件前部的 `MAP_*`（扫描器也要用）。 */

/* `MapObj` 的形状（`M9-design.md` §3.7）。单字指针指向它。
 *
 * ⚠️ **`count` 必须留在 `+0`**（`R-10-19`）：`isel` 的 `len(m)` 就是
 * `ldr x11, [x10]`（`BuiltinKind::Len` 的 map 分支），**没有**调
 * `rt_map_len`。M10 一开始把 `slots` 挪到字 0（想让 gcdata 只有 bit0），
 * 结果 `len(m)` 读出的是槽块指针 —— `indirect.go` 立刻报 `wrong maplen`。
 * 改回来，用 gcdata 的 bit5 指 `slots`。
 */
typedef struct {
    u64 count;          /* +0  活条目数（`len(m)` 读这里，别动） */
    u64 cap;            /* +8  槽数，恒为 2 的幂 */
    u32 key_size;       /* +16 */
    u32 val_size;       /* +20 */
    const TypeDesc *kdesc; /* +24 */
    const TypeDesc *vdesc; /* +32 */
    u8 *slots;          /* +40 槽块（托管，kind = mapslots）的**槽区**起点 */
} MapObj;

/* `MapObj` 的 desc：**只有一个指针字**，在字 5（`slots`）。 */
static const u8 mapobj_gcdata[1] = {0x20};
static const TypeDesc mapobj_desc = {
    sizeof(MapObj), 48, 0, 9 /* Kind::Struct */, 2 /* HAS_POINTERS */, 0,
    mapobj_gcdata, 0, 0};
_Static_assert(__builtin_offsetof(MapObj, count) == 0, "len(m) 读 +0");
_Static_assert(__builtin_offsetof(MapObj, slots) == 40, "gcdata 的 bit5 指这里");

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

/* 分配一个 `cap` 槽的**槽块**（`C12` §2.2 的 `mapslots`），返回**槽区**起点。
 *
 * data 形状：`[+0 kdesc][+8 vdesc][+16 槽…]`。`gc_alloc` 恒清零 ⇒ 槽状态
 * 全是 `MAP_EMPTY`，不必再 memset。
 *
 * ⚠️ 槽块的 `n` 是 **cap** 不是 count：扫的时候要按 `cap` 走（`EMPTY`/`TOMB`
 * 的槽里可能还躺着上一条命的指针 —— `gc_alloc` 清零过，但 `delete` 只把状态
 * 改成 `TOMB`、**不清 key/val**）。按 `count` 扫会漏掉后面的 LIVE 槽。 */
static u8 *map_slots_new(const TypeDesc *kdesc, const TypeDesc *vdesc, u64 cap) {
    u64 stride = 8UL + align8((u64)kdesc->size) + align8((u64)vdesc->size);
    u8 *p = (u8 *)gc_alloc(16 + cap * stride, K_MAPSLOTS, 0, cap);
    *(const TypeDesc **)p = kdesc;
    *(const TypeDesc **)(p + 8) = vdesc;
    return p + 16;
}

/* 分配 `cap` 个槽。 */
static void map_alloc_slots(MapObj *m, u64 cap) {
    m->cap = cap;
    m->slots = map_slots_new(m->kdesc, m->vdesc, cap);
}

/* `make(map[K]V[, n])` —— `n` 只是容量提示，M9 从 8 个槽起步（靠 rehash 长）。
 *
 * # 为什么忽略 `n`
 *
 * 它只影响性能，不影响可观测语义（`len` 只数活条目，`cap` 对 map 不可见）。
 * 按 `n` 预分配反而会让「小 map 大 n」浪费一堆 arena —— 而 M9 没有 GC。
 */
/* 真正的构造函数。
 *
 * # ⚠️ 顺序：**先槽块、后 `MapObj`**（`R-10-18`）
 *
 * `gc_alloc` 在 stress 下会在分配**之前**先跑一次 GC。所以「刚分配出来、
 * 还没写进任何可达位置」的块是**看不见的** —— 如果先建 `MapObj` 再建槽块，
 * 第二次分配触发的 GC 会把 `MapObj` 收掉，返回给 Go 的就是一块死内存。
 *
 * 反过来（先槽块、后 `MapObj`）就安全：`MapObj` 是最后一个分配，它返回后
 * 立刻被调用方存进可达位置，中间不再分配。
 *
 * 这条对 `map_grow` 同样适用：新槽块分配之后只做 memcpy，不分配。 */
static MapObj *map_new(const TypeDesc *kdesc, const TypeDesc *vdesc) {
    u8 *slots = map_slots_new(kdesc, vdesc, 8);
    /* 槽块还没发布 ⇒ pin 住，否则 `MapObj` 那次分配前的 GC 会收掉它。 */
    gc_pin_push(slots);
    MapObj *m = (MapObj *)gc_alloc(sizeof(MapObj), K_OBJECT, &mapobj_desc, 0);
    gc_pin_pop();
    m->slots = slots;
    m->count = 0;
    m->cap = 8;
    m->key_size = kdesc->size;
    m->val_size = vdesc->size;
    m->kdesc = kdesc;
    m->vdesc = vdesc;
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
    /* 先分配新槽块，再改 `m->slots` —— 中间**不分配**（只有 memcpy）。 */
    u8 *new_slots = map_slots_new(m->kdesc, m->vdesc, old_cap * 2);
    m->cap = old_cap * 2;
    m->slots = new_slots;
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
    /* 走托管入口（`C12` §2.2）：GC 靠「元素 desc + 元素个数」扫它。
     * 块 16 对齐 ⇒ 任何 `align <= 16` 的元素都满足。 */
    out->ptr = rt_alloc_array(elem, cap);
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
        np = rt_alloc_array(elem, newcap);
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
    char *q = (char *)rt_alloc_raw(n);
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
    u8 *buf = (u8 *)rt_alloc_raw(n * 4);
    u64 k = 0;
    for (u64 i = 0; i < n; i++) {
        k += utf8_put(buf + k, p[i]);
    }
    out.p = (const char *)buf;
    out.n = k;
    return out;
}


/* `a + b`：结果落在托管堆的 `raw` 块里（`C12` §2.2 —— 里面没有指针，不扫）。 */
rstr rt_strcat(rstr a, rstr b) {
    u64 n = a.n + b.n;
    /* 空结果也要有**可用的地址**（Go 允许 `len(s)==0` 时取 `&s[0]`）。 */
    char *p = (char *)rt_alloc_raw(n == 0 ? 1 : n);
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
 * - 结果落在托管堆（`raw` 块），**即使 1 字节**也要有稳定地址（`&s[0]` 合法）。 */
rstr rt_i64_to_str(i64 v) {
    char *p = (char *)rt_alloc_raw(4);
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

/* defer 链的根（`C12` §4.1 第 3 类）。
 *
 * # 为什么 `DeferRec` 自己不是根
 *
 * 它留在 arena（`K_ARENA`，永不回收），所以不需要标记。但它的 `fv` 指向
 * **托管堆上的闭包块**（`MakeClosure` 走 `rt_alloc_closure`）—— 不扫它，
 * 闭包块会在下一次 GC 被回收，`rt_deferrun` 里的 `call_funcval(fv)` 就读到
 * 垃圾（实测 `closure4.go` / `defernil.go` 在 stress 下 rc=135）。
 *
 * `next` 是 arena 地址、`fp` 是栈地址 —— `mark_ptr` 的 `chunk_of` 都会忽略。
 */
static void mark_defer(void) {
    for (DeferRec *r = g_defer_top; r != 0; r = r->next) {
        mark_ptr(r->fv);
    }
}

void rt_defer_push(void *fp, void *fv) {
    /* ⚠️ `gc_pause` 不是可选的（`G-M10-06` / `R-10-24`）。
     *
     * `rt_arena_alloc` 走 `gc_alloc`，而 `gc_alloc` 的 stress 钩子在**分配前**
     * 跑一次 GC。此刻新记录**还没挂上** `g_defer_top`，所以：
     *
     *   * `mark_defer` 看不到这个闭包；
     *   * `Defer` 不是 safepoint ⇒ `sp_idx` 还停在 `MakeClosure` 那个位点。
     *
     * 于是闭包块的**唯一**根就是 `MakeClosure` 的位图。M10 的位图是 live-out
     * 超集，恰好含 dst，所以一直没露 —— 但那是**巧合**，不是保证：
     * 改成真 live-in 后 `defernil` / `print` / `nilptr2` / `method5` 全部坏掉，
     * 病灶就在这里（闭包被回收 ⇒ 地址被下一个闭包复用 ⇒ `rt_deferrun`
     * 调到**别人的**闭包 ⇒ panic 且 `recover()` 救不回来）。
     *
     * `gc_pause` 的语义正好是「一次分配 + 立即发布」，与 `rt_alloc_escaped`
     * 同一个理由（`R-10-23`）。分配与发布之间没有别的分配点 ⇒ 不收也安全。 */
    gc_pause++;
    DeferRec *r = rt_arena_alloc(sizeof(DeferRec), 8);
    r->next = g_defer_top;
    r->fp = fp;
    r->fv = fv;
    g_defer_top = r;
    gc_pause--;
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
/* goroutine 栈溢出（M11a/Task 10；`C9` §5 / `M11-design.md` §3.4）。
 *
 * ⚠️ **不可 `recover`**：栈已经不够了，展开 defer 会继续踩栈（`C11` §5 的
 * 死锁/panic 都假设还有栈可用）。所以这里**不**走 `panic_set`，而是直接打
 * stderr + 退出 2 —— 与 `rt_fatal_oom` 同一个形状。
 *
 * ⚠️ **`<N>` 必须是真的**：取当前 G 的 `stack_hi - stack_lo`，不是硬编码
 * 65536。理由：`main` 用的是进程栈（M11a 阶段 4 MiB），新 G 才是 64 KiB
 * （`D-M11-2`）—— 写死 65536 会让 `main` 的溢出报出一个假数字，而
 * 「文本与真实限制一致」正是 `T-STK-03` 要钉的东西。
 *
 * ⚠️ **缓冲必须是静态的**：此刻 `sp` 已经在 `stack_lo` 之下，在栈上开
 * `char buf[80]` 可能正好踩进 guard page（那才是真段错误）。`.bss` 里的
 * 80 字节没有这个问题；单线程也不需要可重入。 */
static char g_so_buf[96];

void rt_panic_stack_overflow(void) {
    const char *pre = "runtime error: goroutine stack exceeded ";
    u64 n = 0;
    while (pre[n] != 0) {
        g_so_buf[n] = pre[n];
        n++;
    }
    u64 v = 0;
    if (g_cur != 0) {
        /* ⚠️ 两个 `u64 *` 相减是**按元素个数**算的（C 的指针算术），
         * 直接写得数会是「字节数 / 8」—— 实测把 4 MiB 报成了 512 KiB。
         * 必须先转 `u64`。 */
        v = (u64)g_cur->stack_hi - (u64)g_cur->stack_lo;
    }
    char d[24];
    u64 k = 0;
    if (v == 0) {
        d[k++] = '0';
    }
    while (v > 0) {
        d[k++] = (char)('0' + (v % 10));
        v /= 10;
    }
    while (k > 0) {
        g_so_buf[n++] = d[--k];
    }
    const char *post = " bytes\n";
    u64 i = 0;
    while (post[i] != 0) {
        g_so_buf[n++] = post[i++];
    }
    rt_write(2, g_so_buf, n);
    rt_exit(2);
}

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

/* ════════════════════════════════════════════════════════════════════
 * GC 统计行（M10/Task 3，`C12` §5.2）
 * ════════════════════════════════════════════════════════════════════ */

/* 往 `b` 追加一个字符串 / 十进制数，返回新的长度。
 *
 * 不用 `snprintf`：freestanding 没有 libc（`-nostdlib`）。也不用
 * `rt_print_*`：那些写 **stdout**，而统计行的唯一去向是 stderr（`D-M10-15`）。 */
static u64 sb_str(char *b, u64 n, const char *s) {
    while (*s != 0) {
        b[n++] = *s++;
    }
    return n;
}

static u64 sb_u64(char *b, u64 n, u64 v) {
    char t[20];
    u64 k = 0;
    if (v == 0) {
        b[n++] = '0';
        return n;
    }
    while (v > 0) {
        t[k++] = (char)('0' + (v % 10));
        v /= 10;
    }
    while (k > 0) {
        b[n++] = t[--k];
    }
    return n;
}

static u64 chunk_count(void) {
    u64 n = 0;
    for (Chunk *c = heap; c != 0; c = c->next) {
        n++;
    }
    return n;
}

/* 退出前打**一行**统计（`C12` §5.2 冻结的键序与语法）。
 *
 * # 谁调用它
 *
 * **生成的 `_start`**（`rgoc-arm64::module` 在 `main` 返回之后、判 panic 之前插
 * 一条 `bl rt_gc_stats_line`），不是 libc 的 `atexit` —— freestanding 没有
 * `atexit`，而「自己造一个」要在 `_start` 里维护函数指针表，为一行日志不值得。
 *
 * # 为什么 `gc_stats == 0` 时一个字节都不打
 *
 * 统计行进的是 stderr，而 `C9` §4.1 的合并流只看 stdout —— 但「不设就不打」
 * 仍然是契约（`C12` §5.2）：否则 `RGOC_GC_STATS` 就不是开关，而是
 * 「一直开着」的噪声源。
 */
/* 峰值 RSS（KB），`getrusage(RUSAGE_SELF).ru_maxrss`（`T-GC-09`）。
 *
 * # 为什么是「峰值」而不是「当前」
 *
 * `ru_maxrss` 是**峰值**。判据要的正是峰值：`heap` 是 mmap 量，而真正占物理
 * 内存的是「碰过的页」。一个只分配不写的程序 `heap` 很大而 RSS 很小 ——
 * 两个数一起看才知道有没有真的把内存用起来。
 *
 * # 布局（Linux aarch64）
 *
 * ```text
 * struct rusage { timeval ru_utime(16); timeval ru_stime(16); long ru_maxrss; ... }
 * ```
 * 所以 `ru_maxrss` 在偏移 **32**（第 4 个 `u64`）。内核给的是 **KB**。
 *
 * 取不到就返回 0（字段必须出现，值可以是 0 —— `R-10-2`）。
 */
static u64 rss_kb(void) {
    u64 buf[18];
    for (u64 i = 0; i < 18; i++) {
        buf[i] = 0;
    }
    i64 r = sys3(SYS_getrusage, 0 /* RUSAGE_SELF */, (i64)buf, 0);
    if (r < 0) {
        return 0;
    }
    return buf[4];
}

void rt_gc_stats_line(void) {
    if (gc_stats == 0) {
        return;
    }
    char b[512];
    u64 n = 0;
    n = sb_str(b, n, "gc: collections=");
    n = sb_u64(b, n, gc_collections);
    n = sb_str(b, n, " allocs=");
    n = sb_u64(b, n, gc_allocs);
    n = sb_str(b, n, " frees=");
    n = sb_u64(b, n, gc_frees);
    n = sb_str(b, n, " live=");
    n = sb_u64(b, n, gc_live_bytes);
    n = sb_str(b, n, " heap=");
    n = sb_u64(b, n, heap_committed);
    n = sb_str(b, n, " chunks=");
    n = sb_u64(b, n, chunk_count());
    n = sb_str(b, n, " rss=");
    n = sb_u64(b, n, rss_kb());
    n = sb_str(b, n, " kinds=");
    for (u64 i = 0; i < 5; i++) {
        if (i != 0) {
            b[n++] = ',';
        }
        n = sb_u64(b, n, gc_kind_count[i]);
    }
    n = sb_str(b, n, " bad_sp=");
    n = sb_u64(b, n, gc_bad_sp);
    n = sb_str(b, n, " bad_ptr=");
    n = sb_u64(b, n, gc_bad_ptr);
    n = sb_str(b, n, " bad_desc=");
    n = sb_u64(b, n, gc_bad_desc);
    /* `mode` 是**解析后**的生效态（`C12` §5.2 / `R-10-3`）。
     *
     * # 为什么它必须在行里
     *
     * Phase 0 的三态**必然等价**（`rt_gc()` 还不存在），所以「三态跑出同样的
     * 字节」在 Phase 0 里不携带信息 —— 装置写错了也看不出来。`mode` 让
     * 「环境变量真的生效」变成可观测的：三态各打自己的名字，写错立刻红。
     *
     * 优先级：`RGOC_GC=0` 最高（关了就是关了），其次 `STRESS`，最后 `on`。 */
    n = sb_str(b, n, " mode=");
    if (gc_on == 0) {
        n = sb_str(b, n, "off");
    } else if (gc_stress != 0) {
        n = sb_str(b, n, "stress");
    } else {
        n = sb_str(b, n, "on");
    }
    b[n++] = '\n';
    write_all(FD_STDERR, b, n);
}

/* 探针（`C12` §5.4）：测试只读统计行，但这几个在调试时能直接看。 */
u64 rt_gc_collections(void) { return gc_collections; }
u64 rt_gc_live_bytes(void) { return gc_live_bytes; }
u64 rt_gc_alloc_bytes(void) { return gc_alloc_bytes; }
u64 rt_gc_bad_sp(void) { return gc_bad_sp; }
u64 rt_gc_bad_ptr(void) { return gc_bad_ptr; }
u64 rt_gc_bad_desc(void) { return gc_bad_desc; }

