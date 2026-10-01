import json
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

import tcc_guard as guard

PREFIX = "/Users/person/Library/Caches/soyeht-e2e/0123456789abcdef/bin/"


def log_line(process, message):
    return json.dumps({"processID": process, "eventMessage": message})


class TccGuardTests(unittest.TestCase):
    def test_reads_only_path_records_under_the_prefix(self):
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "TCC.db"
            with closing(sqlite3.connect(database)) as connection:
                connection.execute(
                    "create table access (service text, client text, client_type integer,"
                    " last_modified integer)")
                connection.executemany("insert into access values (?, ?, ?, ?)", [
                    ("kTCCServiceSystemPolicyAppBundles", PREFIX + "soyeht-ptyd", 1, 10),
                    ("kTCCServiceAppleEvents", "/private/tmp/other/soyeht-ptyd", 1, 11),
                    ("kTCCServiceAppleEvents", PREFIX + "bundle.id", 0, 12),
                ])
                connection.commit()
            records = guard.read_records(database, PREFIX)
        self.assertEqual(records, {f"kTCCServiceSystemPolicyAppBundles\t{PREFIX}soyeht-ptyd": 10})

    def test_reports_added_and_modified_records_only(self):
        before = {"a": 1, "b": 2}
        after = {"a": 1, "b": 3, "c": 4}
        self.assertEqual(guard.changed_records(before, after), ["b", "c"])
        self.assertEqual(guard.changed_records(after, after), [])

    def test_joins_request_service_and_subject_for_the_prefix(self):
        lines = [
            log_line(640, "AUTHREQ_CTX: msgID=414.1, function=TCCAccessRequest,"
                          " service=kTCCServiceSystemPolicyAppBundles, preflight=yes"),
            log_line(640, f"AUTHREQ_SUBJECT: msgID=414.1, subject={PREFIX}soyeht-ptyd,"),
            # The developer-tool preflight at every exec is ignored.
            log_line(421, f"AUTHREQ_SUBJECT: msgID=414.2, subject={PREFIX}soyeht-ptyd,"),
            log_line(421, "AUTHREQ_CTX: msgID=414.2, function=TCCAccessRequest,"
                          " service=kTCCServiceDeveloperTool, preflight=yes"),
            log_line(640, "AUTHREQ_CTX: msgID=414.3, service=kTCCServiceAppleEvents,"),
            log_line(640, "AUTHREQ_SUBJECT: msgID=414.3, subject=/Applications/Other.app,"),
            "not json",
        ]
        self.assertEqual(guard.requests_in_log(lines, PREFIX),
                         [f"kTCCServiceSystemPolicyAppBundles for {PREFIX}soyeht-ptyd"])

    def test_an_unreadable_database_skips_the_record_check(self):
        with tempfile.TemporaryDirectory() as directory:
            missing = Path(directory) / "missing.db"
            with self.assertRaises(sqlite3.Error):
                guard.read_records(missing, PREFIX)


if __name__ == "__main__":
    unittest.main()
