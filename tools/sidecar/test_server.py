"""Protocol regressions for the reference sidecar, using real HTTP requests."""
import json
import threading
import unittest
import urllib.request

import server


class Protocol(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.http = server.HTTPServer(('127.0.0.1', 0), server.Handler)
        cls.thread = threading.Thread(target=cls.http.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.http.shutdown()
        cls.thread.join(timeout=5)
        cls.http.server_close()

    def setUp(self):
        server.storage.clear()
        server.logs.clear()

    def rpc(self, path, cell='A', field='data', **extra):
        payload = json.dumps(dict(cell=cell, field=field, **extra)).encode()
        request = urllib.request.Request(
            f'http://127.0.0.1:{self.http.server_port}/{path}', data=payload,
            headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=5) as response:
            return json.load(response)

    def test_namespaces_and_typed_values(self):
        value = {'type': 'list', 'value': [{'type': 'null'}, {'type': 'float', 'value': 'NaN'}]}
        self.assertEqual(self.rpc('set', 'A.B', 'C', key='é', value=value), {'ok': True})
        self.assertEqual(self.rpc('get', 'A.B', 'C', key='é'), {'value': value})
        self.assertEqual(self.rpc('get', 'A', 'B.C', key='é'), {'value': None})
        self.assertEqual(self.rpc('get', 'A.B', 'D', key='é'), {'value': None})

    def test_lists_can_be_undone_to_empty(self):
        value = {'type': 'int', 'value': 42}
        self.assertEqual(self.rpc('append', value=value), {'ok': True})
        self.assertEqual(self.rpc('list'), {'items': [value]})
        self.assertEqual(self.rpc('len'), {'len': 1})
        self.assertEqual(self.rpc('unappend'), {'ok': True})
        self.assertEqual(self.rpc('unappend'), {'ok': True})
        self.assertEqual(self.rpc('list'), {'items': []})
        self.assertEqual(self.rpc('len'), {'len': 0})

    def test_keys_and_values_exclude_metadata_in_matching_order(self):
        for key, number in [('z', 3), ('__meta', 9), ('a', 1)]:
            self.rpc('set', key=key, value={'type': 'int', 'value': number})
        self.assertEqual(self.rpc('keys'), {'keys': ['a', 'z']})
        self.assertEqual(self.rpc('values'), {'values': [
            {'type': 'int', 'value': 1}, {'type': 'int', 'value': 3}]})
        self.assertEqual(self.rpc('len'), {'len': 2})
        self.assertEqual(self.rpc('delete', key='a'), {'deleted': True})
        self.assertEqual(self.rpc('delete', key='a'), {'deleted': False})
        self.assertEqual(self.rpc('has', key='a'), {'exists': False})


if __name__ == '__main__':
    unittest.main()
