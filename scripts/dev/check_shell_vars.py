#!/usr/bin/env python3
"""检查 shell 片段里「变量后面紧跟非 ASCII 字符」的写法。

为什么要有这个检查：bash 5（GitHub runner 上用的就是）会把紧跟在变量名后的多字节字符
（全角逗号「，」、全角括号「（」这类）当成变量名的一部分，于是

    echo "版本 $V，tag $TAG（…）"

在 `set -u` 下直接报 `line N: V，tag: unbound variable`。而 macOS 自带的 /bin/sh 是
bash 3.2，本地怎么测都测不出来 —— 这个坑在 CI 上踩过两次，所以做成发布前的检查。

修法：给变量加大括号 → `${V}`。
"""
import glob
import re
import sys

PAT = re.compile(r"\$([A-Za-z_][A-Za-z0-9_]*)(?=[^\x00-\x7F])")
TARGETS = ["scripts/*.sh", ".github/workflows/*.yml"]

hits = []
for pattern in TARGETS:
    for path in glob.glob(pattern):
        with open(path, encoding="utf-8") as fh:
            for lineno, line in enumerate(fh, 1):
                for m in PAT.finditer(line):
                    hits.append((path, lineno, m.group(0), line.strip()[:110]))

if hits:
    for path, lineno, token, line in hits:
        print(f"{path}:{lineno}: {token}   ← {line}")
    print(
        f"\n共 {len(hits)} 处：变量后面紧跟全角字符，bash 5 下会报 unbound variable。"
        " 请把 $VAR 改成 ${VAR}。"
    )
    sys.exit(1)

print("shell 变量写法检查通过（没有「变量后紧跟全角字符」的写法）")
