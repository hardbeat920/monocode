import argparse
import ctypes
import json
import os
from pathlib import Path
import platform
import subprocess
import sys

ORACLE = r"""
console.log(JSON.stringify({node:process.version,icu:process.versions.icu,
collator_locale:new Intl.Collator().resolvedOptions().locale,
relative_locale:new Intl.RelativeTimeFormat(undefined,{numeric:'auto'}).resolvedOptions().locale,
relative:new Intl.RelativeTimeFormat(undefined,{numeric:'auto'}).format(-2,'hour')}));
"""


def oracle(node, env):
    result = subprocess.run([node, '--eval', ORACLE], env=env, capture_output=True,
                            check=True, timeout=15)
    return json.loads(result.stdout.decode('utf-8'))


def windows_languages():
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    preferred = kernel.GetUserPreferredUILanguages
    preferred.argtypes = [ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong),
                          ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
    preferred.restype = ctypes.c_int
    count = ctypes.c_ulong()
    length = ctypes.c_ulong()
    if not preferred(8, ctypes.byref(count), None, ctypes.byref(length)):
        raise ctypes.WinError(ctypes.get_last_error())
    buffer = ctypes.create_unicode_buffer(length.value)
    if not preferred(8, ctypes.byref(count), buffer, ctypes.byref(length)):
        raise ctypes.WinError(ctypes.get_last_error())
    languages = ''.join(buffer[:length.value]).rstrip('\0').split('\0')
    locale_name = kernel.GetUserDefaultLocaleName
    locale_name.argtypes = [ctypes.c_void_p, ctypes.c_int]
    locale_name.restype = ctypes.c_int
    locale = ctypes.create_unicode_buffer(85)
    if not locale_name(locale, len(locale)):
        raise ctypes.WinError(ctypes.get_last_error())
    return {'GetUserPreferredUILanguages': languages,
            'GetUserPreferredUILanguages_count': count.value,
            'GetUserDefaultLocaleName': locale.value}


parser = argparse.ArgumentParser()
parser.add_argument('--node', required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
rows = [{'case': 'inherited', 'node': oracle(args.node, os.environ.copy())}]
report = {'platform': sys.platform, 'hostname': platform.node(), 'cases': rows}
if sys.platform == 'win32':
    report.update(windows_languages())
elif sys.platform.startswith('linux'):
    for label, changes in [
        ('language-list-with-english-lc-all',
         {'LANGUAGE': 'fr_FR:de_DE', 'LANG': 'en_US.UTF-8', 'LC_ALL': 'en_US.UTF-8'}),
        ('language-french-with-C-lc-all',
         {'LANGUAGE': 'fr_FR', 'LANG': 'C', 'LC_ALL': 'C'}),
    ]:
        env = os.environ.copy()
        for key in list(env):
            if key.startswith('LC_') or key in ['LANG', 'LANGUAGE']:
                env.pop(key)
        env.update(changes)
        rows.append({'case': label, 'child_environment': changes,
                     'node': oracle(args.node, env)})
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_bytes((json.dumps(report, ensure_ascii=False, indent=2) + '\n').encode('utf-8'))
print(json.dumps(report, ensure_ascii=True, indent=2))
