# /// script
# requires-python = ">=3.11"
# dependencies = ["praat-parselmouth==0.4.7", "numpy>=2"]
# ///
"""wav を Praat（Burg 法）で分析し、F1〜F4 の中央値を出す。

`--expect F1,F2,F3,F4` を渡すと、正しい値との差（%）を出し、
`--tolerance` を超えたものがあれば終了コード 1 で終わる。
`--compare <voice-cli analyze の CSV>` を渡すと、同じ wav の voice-cli の推定値との差を出す（フェーズ 2 で使う）。

ファイルを読むだけで、何も書かない。
"""

import argparse
import csv
import json
import statistics
import sys

import parselmouth


def praat_formants(path, ceiling, n_formants, skip):
    sound = parselmouth.Sound(path)
    formant = sound.to_formant_burg(
        time_step=0.01, max_number_of_formants=n_formants, maximum_formant=ceiling
    )
    medians = []
    for i in range(1, 5):
        values = []
        for t in formant.ts():
            if t < skip:
                continue
            v = formant.get_value_at_time(i, t)
            if v == v:  # NaN を除く
                values.append(v)
        medians.append(statistics.median(values) if values else None)
    return medians


def cli_formants(path):
    columns = {f"f{i}": [] for i in range(1, 5)}
    with open(path, newline="") as f:
        for row in csv.DictReader(f):
            for key in columns:
                if row.get(key):
                    columns[key].append(float(row[key]))
    return [statistics.median(v) if v else None for v in columns.values()]


def diff_percent(a, b):
    if a is None or b is None:
        return None
    return (a - b) / b * 100.0


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("wav")
    p.add_argument("--ceiling", type=float, default=5500.0, help="フォルマントの上限（Hz）")
    p.add_argument("--formants", type=float, default=5.0, help="極の数の半分（Praat の max number of formants）")
    p.add_argument("--skip", type=float, default=0.1, help="始めの何秒を除くか")
    p.add_argument("--expect", help="正しい F1〜F4（カンマ区切り）")
    p.add_argument("--compare", help="voice-cli analyze の CSV")
    p.add_argument("--tolerance", default="5,5,5,8", help="F1〜F4 の許す差（%%、カンマ区切り）")
    args = p.parse_args()

    praat = praat_formants(args.wav, args.ceiling, args.formants, args.skip)
    reference = None
    label = None
    if args.expect:
        reference, label = [float(v) for v in args.expect.split(",")], "expected"
        target = praat
    elif args.compare:
        reference, label = praat, "praat"
        target = cli_formants(args.compare)
    else:
        target = praat

    tolerance = [float(v) for v in args.tolerance.split(",")]
    rows, failed = [], False
    for i in range(4):
        row = {"formant": f"F{i + 1}", "value": target[i]}
        if reference is not None:
            d = diff_percent(target[i], reference[i])
            ok = d is not None and abs(d) <= tolerance[i]
            failed |= not ok
            row.update({label: reference[i], "diff_percent": d, "result": "PASS" if ok else "FAIL"})
        rows.append(row)

    for r in rows:
        print("\t".join("" if v is None else (f"{v:.1f}" if isinstance(v, float) else str(v)) for v in r.values()))
    print(json.dumps({"wav": args.wav, "ceiling": args.ceiling, "rows": rows}), file=sys.stderr)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
