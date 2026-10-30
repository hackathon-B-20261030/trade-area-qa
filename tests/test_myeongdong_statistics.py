from pathlib import Path
import json
import tempfile
import unittest

from scripts.prepare_myeongdong_statistics import parse_html_rows, numbers, make_statistics


class StatisticsTests(unittest.TestCase):
    def test_statistics_parsing(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / 'sample.html'
            source.write_text('<meta charset="utf-8"><table><tr><td>지역</td><td>1,234</td></tr><tr><td>ignore</td></tr></table>', encoding='utf8')
            self.assertEqual(parse_html_rows(source, 2), [['지역', '1,234']])
            self.assertEqual(numbers(['1,234', '0']), [1234, 0])
            self.assertEqual(numbers(['224860.0000']), [224860])
            with self.assertRaises(ValueError): numbers(['X'])
            with self.assertRaises(ValueError): numbers(['1.2'])
            with self.assertRaises(ValueError): parse_html_rows(source, 3)

    def test_statistics_real_sources(self):
        p = Path('/private/tmp/cbu_myeongdong_stats_research')
        paths = [p/'mois_age_202609_gu_5.html',p/'mois_age_202609_gu_19.html',p/'mois_single_202609_city.html',p/'mois_age_202609_junggu_dong.html',p/'marriage_download.bin']
        if not all(x.exists() for x in paths):
            saved = Path('data/myeongdong-augmentation-inputs/statistics.json')
            if not saved.exists(): self.skipTest('Public source responses not available locally')
            data = json.loads(saved.read_text(encoding='utf8'))
        else:
            data = make_statistics(*paths)
        self.assertEqual(len(data['population']), 850)
        self.assertEqual(sum(c['population'] for c in data['population']), 8134274)
        self.assertEqual(sum(c['single_registered'] for c in data['population']), 2008370)
        self.assertEqual(len(data['marriage']), 28)
        self.assertEqual(len(data['district_marriage']), 659)
        self.assertEqual(len(data['suppressed_district_marriage']), 41)
        self.assertTrue(all(m['total'] == m['never'] + m['married'] + m['divorced_widowed'] for m in data['marriage']))
        self.assertTrue(all(m['total'] == m['never'] + m['married'] + m['divorced_widowed'] for m in data['district_marriage']))
        self.assertTrue(all(set(m) == {'district','sex','age_lower'} for m in data['suppressed_district_marriage']))
        self.assertEqual(data['myeongdong_context']['code'], '1114055000')
        self.assertEqual(data['myeongdong_context']['all_ages_total'], 2373)
        self.assertTrue(all(len(source['raw_response_sha256']) == 64 for source in data['sources']))

if __name__ == '__main__': unittest.main()
