package p

import l "lib"

var _ = l.Exported
var _ = l.hidden
var _ = l.Nope
var _ = l.exported
var _ = l.T{}
var _ = l.V
var _ = l.F
var _ = l.g
var _ int = l.T{}.M()
var _ int = l.T{}.m()
