package main

import (
	"flag"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

// -update 重写 .want。只在**有意**的 dump 变更后手工运行；
// 平时它必须失败，否则 golden 就从判据退化成「当前输出的抄本」。
var update = flag.Bool("update", false, "重写 testdata/*.want（有意的 dump 变更才用）")

// T-TY-91（oracle 侧）：testdata 下每个 .go 的 dump 与同名 .want **逐字节一致**。
//
// 几条纪律：
//  1. **逐字节**（不是逐行 trim）：空白/换行也是契约（C7 §2：LF、末尾恰一个换行）；
//  2. fixture **必须合法**（检查器零错）：不合法的话 dumps 的一半是 `invalid`，
//     那种「对撞」没有意义（`M3-tests.md` §5.1 的「不进对撞」一条同理）；
//  3. 每个 .go 都要有 .want：缺了就是漏写，不是「新 fixture 待生成」（用 -update）。
func TestDumpGolden(t *testing.T) {
	fixtures, err := filepath.Glob("testdata/*.go")
	if err != nil {
		t.Fatal(err)
	}
	if len(fixtures) == 0 {
		t.Fatal("testdata 里没有 .go —— golden 测试什么都没测")
	}
	for _, fx := range fixtures {
		fx := fx
		t.Run(filepath.Base(fx), func(t *testing.T) {
			got, errs, fatal := checkFiles("", []string{fx})
			if fatal != nil {
				t.Fatalf("解析/读取失败：%v", fatal)
			}
			if len(errs) > 0 {
				t.Fatalf("fixture 必须是**合法**程序，检查器报了 %d 条错（首条：%v）", len(errs), errs[0])
			}
			want := strings.TrimSuffix(fx, ".go") + ".want"
			if *update {
				if err := os.WriteFile(want, []byte(got), 0o644); err != nil {
					t.Fatal(err)
				}
				return
			}
			w, err := os.ReadFile(want)
			if err != nil {
				t.Fatalf("读不到 %s（fixture 新增后要跑一次 `go test ./... -update`）", want)
			}
			if got != string(w) {
				t.Errorf("dump 与 %s 不一致：\n%s", want, firstDiff(string(w), got))
			}
		})
	}
}

// 曾经崩过的三种约束形状必须能出 dump（`number` 的两个 bug 的回归钉）。
//
// 为什么单独钉：它们**不是** dump 是否正确的判据，而是「oracle 能不能用」的
// 判据 —— 崩了的话对撞根本跑不起来（M3 早期实测：`[T int]` 报
// `C7 §3 违反：类型 *types.Union 未编号就被引用`；`[T interface{ M() }]` 报
// 「类型图成环」）。golden 测试本身也会崩，但崩在 `t.Run` 之外的 goroutine
// 里只会给出一个 panic 栈 —— 这里给出一条指名道姓的失败。
func TestConstraintShapesDoNotPanic(t *testing.T) {
	const src = `package p

type C interface{ comparable }

func fInt[T int](x T)                    {}
func fIface[T interface{ String() string }](x T) {}
func fNamed[T C](x T)                    {}
`
	dir := t.TempDir()
	f := filepath.Join(dir, "p.go")
	if err := os.WriteFile(f, []byte(src), 0o644); err != nil {
		t.Fatal(err)
	}
	got, errs, fatal := checkFiles("", []string{f})
	if fatal != nil {
		t.Fatalf("检查失败：%v", fatal)
	}
	if len(errs) > 0 {
		t.Fatalf("这段源码必须合法，检查器报了 %d 条错（首条：%v）", len(errs), errs[0])
	}
	for _, want := range []string{"typeparam", "union", "term"} {
		if !strings.Contains(got, want) {
			t.Errorf("dump 里应当出现 %q：\n%s", want, got)
		}
	}
}

// firstDiff 报首个不同行（两侧各带一行上下文）—— 逐字节失败时比整份 diff 有用。
func firstDiff(want, got string) string {
	wl := strings.Split(want, "\n")
	gl := strings.Split(got, "\n")
	n := len(wl)
	if len(gl) < n {
		n = len(gl)
	}
	for i := 0; i < n; i++ {
		if wl[i] != gl[i] {
			return "首个差异在第 " + strconv.Itoa(i+1) + " 行：\n  want: " + wl[i] + "\n  got : " + gl[i]
		}
	}
	return "前 " + strconv.Itoa(n) + " 行相同，但行数不同：want " + strconv.Itoa(len(wl)) + " 行 / got " + strconv.Itoa(len(gl)) + " 行"
}
