package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// T-TY-92/93（oracle 侧）：`testdata/cmp-*.go` 的判定行与同名 `.want` 逐字节一致。
//
// 与 `TestDumpGolden` 同一套纪律（逐字节 / fixture 必须合法 / 缺 `.want` 报错），
// 分开跑的理由只有一个：一个 fixture 只能有一份 `.want`，而 dump 与 cmp 是
// **两个比对面**。`TestTestdataNaming` 守住「testdata 里每个 .go 都属于某一族」。
func TestCmpGolden(t *testing.T) {
	fixtures, err := filepath.Glob("testdata/cmp-*.go")
	if err != nil {
		t.Fatal(err)
	}
	if len(fixtures) == 0 {
		t.Fatal("testdata 里没有 cmp-*.go —— golden 测试什么都没测")
	}
	for _, fx := range fixtures {
		fx := fx
		t.Run(filepath.Base(fx), func(t *testing.T) {
			got, errs, fatal := checkFiles("", []string{fx}, "cmp")
			if fatal != nil {
				t.Fatalf("解析/读取/指令失败：%v", fatal)
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
				t.Errorf("cmp 输出与 %s 不一致：\n%s", want, firstDiff(string(w), got))
			}
		})
	}
}

// testdata 里的每个 .go 都必须属于某一族（`type-dump-*.go` 或 `cmp-*.go`）。
//
// 两个 golden 测试各自只 glob 自己那一族 ⇒ 一个命名不合规矩的 fixture
// **两边都不测**，却安安静静躺在那里。这条把它顶出来。
func TestTestdataNaming(t *testing.T) {
	all, err := filepath.Glob("testdata/*.go")
	if err != nil {
		t.Fatal(err)
	}
	if len(all) == 0 {
		t.Fatal("testdata 里没有 .go")
	}
	for _, fx := range all {
		base := filepath.Base(fx)
		if strings.HasPrefix(base, "type-dump-") || strings.HasPrefix(base, "cmp-") {
			continue
		}
		t.Errorf("%s 不属于任何一族（要以 `type-dump-` 或 `cmp-` 开头），两个 golden 都不会跑到它", base)
	}
}

// 指令**格式错要响亮地炸**，不能静默跳过（静默跳过 = 样本悄悄消失）。
//
// 反向钉：这条测试断言的就是「有错」，所以它失败只可能是我们把它写成了
// 「不报错」（例如把 parse 错误吞掉改成 continue）。
func TestCmpMalformedDirectiveFailsLoudly(t *testing.T) {
	cases := []struct {
		name, src, wantSub string
	}{
		{"两个分隔符", "package p\n\n//rgoc:cmp identical int => int => int\n", "只能有一个"},
		{"未知 kind", "package p\n\n//rgoc:cmp assignable int => int\n", "未知的 cmp kind"},
		{"空操作数", "package p\n\n//rgoc:cmp identical int =>\n", "空的操作数"},
		{"缺操作数", "package p\n\n//rgoc:cmp identical\n", "缺少操作数"},
		{"没有 => 且段数不对", "package p\n\n//rgoc:cmp identical int\n", "恰好两个"},
		{"三段", "package p\n\n//rgoc:cmp identical int int int\n", "恰好两个"},
		{"A 不是类型表达式", "package p\n\n//rgoc:cmp identical 1+1 => int\n", "不是类型表达式"},
		{"常量不是常量表达式", "package p\n\nvar v = 1\n\n//rgoc:cmp representable v => int8\n", "不是常量表达式"},
		{"目标不是类型表达式", "package p\n\n//rgoc:cmp representable 1 => 1+1\n", "不是类型表达式"},
		{"没有指令", "package p\n", "没有 `//rgoc:cmp` 指令"},
	}
	for _, c := range cases {
		c := c
		t.Run(c.name, func(t *testing.T) {
			dir := t.TempDir()
			f := filepath.Join(dir, "p.go")
			if err := os.WriteFile(f, []byte(c.src), 0o644); err != nil {
				t.Fatal(err)
			}
			_, _, fatal := checkFiles("", []string{f}, "cmp")
			if fatal == nil {
				t.Fatalf("这条指令必须被**判为格式/样本错**，但 oracle 没报错（样本会悄悄消失）")
			}
			if !strings.Contains(fatal.Error(), c.wantSub) {
				t.Errorf("报错里应当出现 %q，实际：%v", c.wantSub, fatal)
			}
		})
	}
}
