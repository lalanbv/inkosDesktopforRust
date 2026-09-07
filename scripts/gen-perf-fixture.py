#!/usr/bin/env python3
"""长篇性能压测 fixture 生成器（213 号）。

生成合成书（N 章 × 3000 中文字 + index + timeline + runtime 工件），
供引擎规模压测复跑：

    python3 scripts/gen-perf-fixture.py <project_root> <book_id> <chapters>

213 号基准结论（M 系列 arm64，engine bin 直启，预热后稳态）：
- 60 章书（3.1MB）：books/detail/timeline/chapter/doctor/analytics 1–3ms；export TXT 542KB 3.4ms
- 500 章书（12MB）：books 列表稳态 8ms（冷读 275ms=磁盘 page cache）；detail 10ms；
  timeline 2.8ms；analytics 1.6ms；doctor 2ms；export TXT 4.5MB 20ms
- 500 条 index JSON 解析 ~0.3ms/次——list_books 逐书聚合非瓶颈

观察（非缺陷，不预优化）：list_books 每次全量读各书 index 聚合
chaptersWritten——单用户桌面上百本书场景才会显化（500 章×10 本≈80ms）。
"""
import json
import os
import random
import sys


def main() -> None:
    root, book_id, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
    book = os.path.join(root, "books", book_id)
    ch = os.path.join(book, "chapters")
    os.makedirs(ch, exist_ok=True)
    os.makedirs(os.path.join(book, "story", "runtime"), exist_ok=True)

    random.seed(42)
    words = "夜港风云潮起潮落暗流涌动破局而立风声鹤唳灯火阑珊剑拔弩张运筹帷幄"
    index = []
    for i in range(1, n + 1):
        title = f"第{i}章·风云变幻"
        with open(os.path.join(ch, f"{i:04d}.md"), "w", encoding="utf-8") as f:
            f.write(f"# {title}\n\n" + "".join(random.choices(words, k=3000)) + "\n")
        index.append({
            "number": i, "title": title,
            "status": "approved" if i < n else "ready-for-review",
            "wordCount": 3000,
            "createdAt": "2026-09-01T00:00:00Z", "updatedAt": "2026-09-01T00:00:00Z",
        })
    with open(os.path.join(ch, "index.json"), "w", encoding="utf-8") as f:
        json.dump(index, f, ensure_ascii=False)
    with open(os.path.join(book, "book.json"), "w", encoding="utf-8") as f:
        json.dump({
            "id": book_id, "title": f"压测书{book_id}", "platform": "other",
            "genre": "urban", "status": "active", "targetChapters": n * 2,
            "chapterWordCount": 3000, "language": "zh",
            "createdAt": "2026-09-01T00:00:00Z", "updatedAt": "2026-09-07T00:00:00Z",
            "version": 1,
        }, f, ensure_ascii=False)
    cells_main = [{"chapter": i, "title": f"主线{i}", "note": "n" * 40} for i in range(1, n + 1)]
    cells_love = [{"chapter": i, "title": f"感情{i}"} for i in range(1, max(2, n // 2))]
    with open(os.path.join(book, "story", "timeline.json"), "w", encoding="utf-8") as f:
        json.dump({
            "version": 1, "bookId": book_id, "updatedAt": "2026-09-07T00:00:00Z",
            "plotlines": [
                {"id": "main", "name": "主线", "cells": cells_main},
                {"id": "love", "name": "感情线", "cells": cells_love},
            ],
        }, f, ensure_ascii=False)
    rd = os.path.join(book, "story", "runtime")
    for i in range(1, n + 1):
        with open(os.path.join(rd, f"chapter-{i:04d}.run.json"), "w", encoding="utf-8") as f:
            json.dump({"chapter": i, "status": "ok", "tokens": 8000}, f)
        with open(os.path.join(rd, f"chapter-{i:04d}.trace.json"), "w", encoding="utf-8") as f:
            json.dump({"chapter": i, "steps": [{"agent": "writer", "messages": ["x" * 500] * 8}] * 9}, f)
    inkos = os.path.join(root, "inkos.json")
    if not os.path.exists(inkos):
        with open(inkos, "w", encoding="utf-8") as f:
            json.dump({"name": "inkos-perf", "version": "0.1.0", "language": "zh"}, f, ensure_ascii=False)
    print(f"ready: {book} ({n} chapters)")


if __name__ == "__main__":
    main()
