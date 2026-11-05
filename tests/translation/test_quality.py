# -*- coding: utf-8 -*-
"""Synthetic gate fixtures, never assertions of live model correctness."""
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('quality', Path(__file__).resolve().parents[2] / 'scripts/dev/check_translation_quality.py')
quality = importlib.util.module_from_spec(spec)
spec.loader.exec_module(quality)


def reviews(**changes):
    result = {role: dict(reviewer='fixture-' + role, meaning_ok=True,
                        target_language_ok=True, part_of_speech_ok=True, one_core_sense_per_line_ok=True, no_extra_content=True)
              for role in ('implementation', 'independent_quality')}
    result['independent_quality'].update(changes)
    return result


class WordContract(unittest.TestCase):
    def check(self, raw, expected='PASS', variants=('/ˈrek.ɔːd/', '/ˈrek.ɚd/'), review=None):
        value = quality.assess_word(raw, variants, reviews() if review is None else review)
        self.assertEqual(value['status'], expected, value)

    def test_compliant_no_pronunciation(self):
        self.check('- n. 记录')

    def test_single_and_double_meanings(self):
        self.check('- n. 记录\n- v. 录制')
        self.check('- n. 记录')

    def test_british_and_american_variants(self):
        self.check('读音：/ˈrek.ɔːd/\n- n. 记录')
        self.check('读音：/ˈrekɚd/\n- n. 记录')

    def test_wrong_pronunciation_preserves_stress_and_phoneme_order(self):
        self.check('读音：/rekˈɔːd/\n- n. 记录', 'FAIL')
        self.check('读音：/ˈrekrɚd/\n- n. 记录', 'FAIL')

    def test_unknown_pronunciation_is_not_a_pass(self):
        self.check('读音：/unverified/\n- n. 记录', 'UNREVIEWED', variants=())

    def test_only_pronunciation(self):
        self.check('读音：/ˈrek.ɔːd/', 'FAIL')

    def test_pronunciation_on_second_line_and_legacy_prefix_fail_format(self):
        self.check('- n. 记录\n读音：/ˈrek.ɔːd/', 'FAIL')
        self.check('音标：/ˈrek.ɔːd/\n- n. 记录', 'FAIL')

    def test_tail_summary_and_third_definition(self):
        self.check('- n. 记录\n总之，这是一个单词。', 'FAIL')
        self.check('- n. 记录\n- v. 录制\n- adj. 创纪录的', 'FAIL')

    def test_wrong_language_or_examples_fail_even_with_valid_line_count(self):
        self.check('- n. a written account', 'FAIL', review=reviews(target_language_ok=False))
        self.check('- 例句：This is a record.', 'FAIL', review=reviews(no_extra_content=False))

    def test_length_and_multiple_slashes(self):
        self.check('- ' + '字' * 81, 'FAIL')
        self.check('- n. 记录/唱片／纪录', 'FAIL')
        self.check('- n. 记录/纪录')

    def test_equivalent_new_wording_requires_two_explicit_reviews(self):
        self.check('- n. 留存的信息', review=reviews())
        self.check('- n. 留存的信息', 'UNREVIEWED', review={})
        self.check('- n. 留存的信息', 'UNREVIEWED', review=reviews(reviewer='fixture-implementation'))

    def test_missing_part_of_speech_is_a_failure(self):
        self.check('- 光；光线', 'FAIL', review=reviews(part_of_speech_ok=False))

    def test_distinct_senses_cannot_share_one_definition_line(self):
        self.check('- adj. 轻的；明亮的', 'FAIL', review=reviews(one_core_sense_per_line_ok=False))

    def test_semantic_drift_is_not_an_allowed_variant(self):
        self.check('- n. 银行', 'FAIL', review=reviews(meaning_ok=False))


class BatchGate(unittest.TestCase):
    def test_a_core_example_response_blocks_even_if_other_cases_pass(self):
        rows = [{'group': 'core', 'status': 'PASS'}, {'group': 'core', 'status': 'FAIL', 'body': '- 例句：fixture'}]
        self.assertEqual(quality.batch_decision([rows]), 'REVISE_PROMPT')
        self.assertEqual(quality.batch_decision([rows] * 3), 'STOP_BLOCKED')
        with self.assertRaises(ValueError):
            quality.batch_decision([rows] * 4)

    def test_missing_repeats_regrouping_duplicates_and_deleted_cases_do_not_pass(self):
        cases = [dict(id='word-01', group='core'), dict(id='limit-01', group='known-limitation')]
        rows = [dict(case_id=c['id'], group=c['group'], repeat=r, status='PASS' if c['group'] == 'core' else 'KNOWN_LIMITATION', classification_unchanged=True, behavior_unchanged=True) for c in cases for r in range(1, 4)]
        self.assertEqual(quality.batch_decision([rows], cases), 'PASS_CORE_AND_BEHAVIOR')
        self.assertEqual(quality.batch_decision([rows[:-1]], cases), 'UNREVIEWED')
        missing = [dict(row) for row in rows]; missing[0].pop('status')
        self.assertEqual(quality.batch_decision([missing], cases), 'UNREVIEWED')
        with self.assertRaises(ValueError):
            quality.batch_decision([rows + [rows[0]]], cases)
        changed = [dict(row) for row in rows]; changed[0]['group'] = 'known-limitation'
        with self.assertRaises(ValueError):
            quality.batch_decision([changed], cases)

    def test_known_limit_semantic_failure_is_preserved_and_not_a_content_pass(self):
        limitation = dict(group='known-limitation', status='KNOWN_LIMITATION', meaning_ok=False,
                          classification_unchanged=True, behavior_unchanged=True)
        self.assertEqual(quality.batch_decision([[dict(group='core', status='PASS'), limitation]]), 'PASS_CORE_AND_BEHAVIOR')
        self.assertIs(limitation['meaning_ok'], False)
        self.assertEqual(limitation['status'], 'KNOWN_LIMITATION')
        limitation['behavior_unchanged'] = False
        self.assertEqual(quality.batch_decision([[dict(group='core', status='PASS'), limitation]]), 'REVISE_PROMPT')


if __name__ == '__main__':
    unittest.main()
