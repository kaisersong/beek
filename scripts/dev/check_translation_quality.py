#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Offline acceptance only; never imported by BeeK or used to alter a result.

Shape/pronunciation checks cannot decide meaning. Two explicit human review
records are required for semantic, language and extra-content approval. Missing
sources/reviews stay UNREVIEWED. Known limitations never count as correct content.
"""
import argparse
import json
import re
from pathlib import Path


def normalize_pronunciation(value):
    value = value.strip()
    if value.startswith('/') and value.endswith('/'):
        value = value[1:-1]
    return re.sub(r'[\s.·]', '', value)


def assess_word(raw, accepted_pronunciations, reviews):
    lines = [line.strip() for line in raw.splitlines() if line.strip()]
    failures = []
    pending = []
    if lines and lines[0].startswith('读音：'):
        pronunciation = lines.pop(0)[len('读音：'):].strip()
        if not pronunciation:
            failures.append('empty pronunciation')
        elif not accepted_pronunciations:
            pending.append('pronunciation source/variant not verified')
        elif normalize_pronunciation(pronunciation) not in {
            normalize_pronunciation(p) for p in accepted_pronunciations
        }:
            failures.append('pronunciation not in frozen variants')
    if not 1 <= len(lines) <= 2:
        failures.append('requires 1–2 definitions')
    for line in lines:
        if not line.startswith('- ') or not line[2:].strip():
            failures.append('definition prefix/body')
        elif len(line[2:]) > 80 or line[2:].count('/') + line[2:].count('／') > 1:
            failures.append('definition length/slashes')
    for role in ('implementation', 'independent_quality'):
        review = reviews.get(role)
        if not review or not review.get('reviewer'):
            pending.append(role + ' review missing')
            continue
        for item in ('meaning_ok', 'target_language_ok', 'part_of_speech_ok', 'one_core_sense_per_line_ok', 'no_extra_content'):
            if review.get(item) is False:
                failures.append(role + ': ' + item)
            elif review.get(item) is not True:
                pending.append(role + ': ' + item)
    if (reviews.get('implementation', {}).get('reviewer') and
            reviews.get('implementation', {}).get('reviewer') ==
            reviews.get('independent_quality', {}).get('reviewer')):
        pending.append('independent reviewer must be different')
    return dict(status='FAIL' if failures else 'UNREVIEWED' if pending else 'PASS',
                failures=failures, pending=pending)


def batch_decision(batches, expected_cases=None, repeats=3):
    """No averaging, regrouping or a fourth candidate after three failures."""
    if not batches:
        return 'UNREVIEWED'
    if len(batches) > 3:
        raise ValueError('at most three candidate batches; stop and preserve failures')
    if expected_cases is not None:
        expected = {(c['id'], repeat): c['group'] for c in expected_cases for repeat in range(1, repeats + 1)}
        for batch in batches:
            seen = {}
            for row in batch:
                key = (row.get('case_id'), row.get('repeat'))
                if key in seen or key not in expected or row.get('group') != expected[key]:
                    raise ValueError('duplicate, unknown or regrouped frozen case')
                seen[key] = row
            if set(seen) != set(expected):
                return 'UNREVIEWED'
    last = batches[-1]
    core = [r for r in last if r['group'] == 'core']
    limits = [r for r in last if r['group'] == 'known-limitation']
    if not core or any(r.get('status') in (None, 'UNREVIEWED') for r in last):
        return 'UNREVIEWED'
    if (all(r.get('status') == 'PASS' for r in core) and
            all(r.get('classification_unchanged') is True and
                r.get('behavior_unchanged') is True for r in limits)):
        return 'PASS_CORE_AND_BEHAVIOR'  # not a semantic pass for the limitations
    return 'STOP_BLOCKED' if len(batches) == 3 else 'REVISE_PROMPT'


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('batch_reviews', type=Path, help='JSON list of complete per-candidate review lists')
    p.add_argument('--corpus', type=Path, default=Path(__file__).resolve().parents[2] / 'tests/fixtures/minimal_translation_corpus.json')
    args = p.parse_args()
    cases = json.loads(args.corpus.read_text(encoding='utf-8'))['cases']
    value = batch_decision(json.loads(args.batch_reviews.read_text(encoding='utf-8')), cases)
    print(value)
    raise SystemExit(0 if value == 'PASS_CORE_AND_BEHAVIOR' else 1)


if __name__ == '__main__':
    main()
