import unittest
from status_select import select_status, StatusError


class T(unittest.TestCase):
    def test_older_status_rows_above_do_not_count(self):
        rows = ['Cellos > echo st=$?', 'st=0', 'Cellos > cat x', 'Cellos > echo st=$?', 'st=130', 'Cellos >']
        self.assertEqual(select_status(rows), 130)

    def test_first_status_with_prior_history(self):
        self.assertEqual(select_status(['st=0', 'Cellos > echo st=$?', 'st=0', 'Cellos > ']), 0)

    def test_missing_echo_fails_closed(self):
        with self.assertRaises(StatusError):
            select_status(['st=0', 'Cellos > '])

    def test_no_status_after_echo_fails_closed(self):
        with self.assertRaises(StatusError):
            select_status(['st=0', 'Cellos > echo st=$?', 'Cellos > '])

    def test_duplicate_status_after_echo_fails_closed(self):
        with self.assertRaises(StatusError):
            select_status(['Cellos > echo st=$?', 'st=0', 'st=1'])

    def test_status_embedded_in_other_text_is_not_a_row(self):
        with self.assertRaises(StatusError):
            select_status(['Cellos > echo st=$?', 'result st=0'])


if __name__ == '__main__':
    unittest.main()
