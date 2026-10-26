#!/usr/bin/env python3
"""Identity mapping and verification for tools/prepare-public.sh (docs/PUBLIC.md).

Usage: tools/public-copy.py mapping <mailmap> <replacements>
       tools/public-copy.py verify <copy> <mailmap> <exclude-list> <allow-list>

`mapping` writes to <replacements> the filter-repo --replace-message/--replace-text expressions
that turn every case, accent and German transliteration variant of an old name or e-mail of the
mailmap into its public one. <replacements> must not leave a temporary directory.

`verify` checks every path of every commit of the copy against the path lists and every
reachable object against the mailmap, prints warnings, findings and a summary, and exits 1 on
any finding.
"""
import re
import subprocess
import sys
import unicodedata

SPELLED_OUT = {'ä': 'ae', 'ö': 'oe', 'ü': 'ue', 'ß': 'ss'}
UMLAUTS = {spelled: umlaut for umlaut, spelled in SPELLED_OUT.items()}
LATIN = [chr(c) for c in range(0x20, 0x7f)] + [chr(c) for c in range(0xc0, 0x250)]
ORIGINAL_DATA = re.compile(r'\.(vga|cp|pal|man|dat|voc|cmf|exe|t|v)$', re.I)
EMAIL = re.compile(r'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}')
MAILMAP_ENTRY = re.compile(r'^\s*([^<]*?)\s*<([^>]*)>\s*(?:([^<]*?)\s*<([^>]*)>)?\s*$')
IDENTITY = re.compile(rb'^(?:author|committer|tagger) (.*) <([^>]*)>', re.M)
TRAILER = re.compile(rb'^[A-Za-z-]+-by:\s*(.*?)\s*<([^>]*)>', re.M | re.I)


def fold(text):
    """Lower case without accents: 'Müller' -> 'muller'."""
    return ''.join(c for c in unicodedata.normalize('NFKD', text)
                   if not unicodedata.combining(c)).casefold()


def spellings(text):
    """`text` as a list of segments, each the tuple of its spellings: an umlaut and its
    spelled-out form stand for each other, written either way: 'Mü' -> [('M',), ('ü', 'ue')]."""
    segments, i = [], 0
    while i < len(text):
        pair = text[i:i + 2].lower()
        if text[i].lower() in SPELLED_OUT:
            segments.append((text[i], SPELLED_OUT[text[i].lower()]))
            i += 1
        elif pair in UMLAUTS:
            segments.append((text[i:i + 2], UMLAUTS[pair]))
            i += 2
        else:
            segments.append((text[i],))
            i += 1
    return segments


def alternation(alternatives):
    # Longest first, so that a replacement at the end of a match takes the whole spelled-out form.
    alternatives = sorted(set(alternatives), key=lambda a: (-len(a), a))
    return alternatives[0] if len(alternatives) == 1 else '(?:' + '|'.join(alternatives) + ')'


def folded_pattern(text):
    """A regex over fold()ed text matching every spelling of `text`: 'Müller' and 'Mueller'
    both match 'muller' and 'mueller'."""
    return re.compile(''.join(alternation(re.escape(fold(s)) for s in segment)
                              for segment in spellings(text)))


def variants_pattern(text):
    """A regex matching every spelling of `text` in any case and accent variant."""
    def char_variants(ch):
        return alternation(re.escape(v) for v in [ch, *(v for v in LATIN if fold(v) == fold(ch))])

    return ''.join(alternation(''.join(char_variants(c) for c in s) for s in segment)
                   for segment in spellings(text))


def read_list(path):
    """The entries of a path list, skipping blank and `#` lines as --paths-from-file does."""
    with open(path, encoding='utf-8') as f:
        return [line for line in f.read().splitlines() if line and not line.startswith('#')]


def read_mailmap(mailmap):
    """(replacements, allowed e-mails, old strings, public identity) of the mailmap."""
    replacements, allowed, old_strings, public = [], set(), set(), None
    with open(mailmap, encoding='utf-8') as f:
        for number, line in enumerate(f, 1):
            line = line.split('#', 1)[0]
            if not line.strip():
                continue
            m = MAILMAP_ENTRY.match(line)
            if not m:
                sys.exit(f'prepare-public: {mailmap}:{number} is not a mailmap entry')
            new_name, new_email, old_name, old_email = m.groups()
            allowed.add(new_email.casefold())
            if public is None and new_name:
                public = (new_name, new_email)
            if old_email is None:
                continue
            old_strings.add(old_email)
            replacements.append((old_email, new_email))
            if old_name:
                old_strings.add(old_name)
                replacements.append((old_name, new_name or old_name))
    if not old_strings:
        sys.exit(f'prepare-public: {mailmap} maps no old identity')
    if public is None:
        sys.exit(f'prepare-public: {mailmap} has no entry with a public name')
    # E-mails first, so that a name inside an e-mail is replaced as part of the e-mail.
    replacements.sort(key=lambda r: '@' not in r[0])
    return replacements, allowed, sorted(old_strings), public


def mapping(mailmap, replacements_file):
    replacements, _, _, _ = read_mailmap(mailmap)
    with open(replacements_file, 'w', encoding='utf-8') as f:
        f.writelines(f'regex:{variants_pattern(old)}==>{new.replace(chr(92), chr(92) * 2)}\n'
                     for old, new in replacements)
    return 0


def verify(copy, mailmap, exclude_list, allow_list):
    def git(*args, data=None, ok=(0,)):
        result = subprocess.run(['git', '-C', copy, *args], input=data, capture_output=True)
        if result.returncode not in ok:
            sys.exit(f'prepare-public: git {" ".join(args)} failed: '
                     f'{result.stderr.decode("utf-8", "replace").strip()}')
        return result.stdout

    excluded = [p.rstrip('/') for p in read_list(exclude_list)]
    allowed_paths = set(read_list(allow_list))
    _, allowed_emails, old_strings, _ = read_mailmap(mailmap)
    old_patterns = [folded_pattern(old) for old in old_strings]
    findings, warnings = [], []

    def is_excluded(path):
        return any(path == p or path.startswith(p + '/') for p in excluded)

    def allowed_email(email):
        email = email.casefold()
        local, _, domain = email.rpartition('@')
        return (email in allowed_emails or local == 'noreply'
                or domain == 'users.noreply.github.com')

    # Every path of every commit: rev-list --objects names each object under one path only.
    paths = git('log', '--all', '--root', '--format=', '--name-only', '--no-renames',
                '--diff-merges=separate', '-z').split(b'\0')
    for path in sorted({p.decode('utf-8', 'replace') for p in paths if p}):
        if is_excluded(path):
            findings.append(f'excluded path in the history: {path}')
        elif ORIGINAL_DATA.search(path) and path not in allowed_paths:
            findings.append(f'original-data file in the history: {path}')

    oids = [line.split(b' ', 1)[0] for line in git('rev-list', '--all', '--objects').splitlines()]
    batch = git('cat-file', '--batch', data=b''.join(oid + b'\n' for oid in oids))
    pos, counts = 0, {}
    while pos < len(batch):
        header_end = batch.index(b'\n', pos)
        oid, kind, size = batch[pos:header_end].decode().split()
        body = batch[header_end + 1:header_end + 1 + int(size)]
        pos = header_end + 1 + int(size) + 1
        counts[kind] = counts.get(kind, 0) + 1
        if kind in ('commit', 'tag'):
            header, _, message = body.partition(b'\n\n')
            for source, regex in ((header, IDENTITY), (message, TRAILER)):
                for name, email in regex.findall(source):
                    name, email = name.decode('utf-8', 'replace'), email.decode('utf-8', 'replace')
                    if not allowed_email(email):
                        findings.append(f'unmapped identity in {kind} {oid}: {name} <{email}>')
        text = fold(body.decode('utf-8', 'replace'))
        hint = f' (git log --all --find-object={oid} names its commits)' \
            if kind in ('blob', 'tree') else ''
        for number, pattern in enumerate(old_patterns, 1):
            if pattern.search(text):
                findings.append(f'old identity string #{number} of the mailmap in {kind} {oid}{hint}')

    heads = git('for-each-ref', '--format=%(refname:short)', 'refs/heads').decode().split()
    tags = git('for-each-ref', '--format=%(refname:short)', 'refs/tags').decode().split()
    for head in heads:
        # git grep exits 1 when nothing matches.
        hits = git('grep', '-n', '-I', '-E', '-e', '/home/', '-e', '/media/', '-e', EMAIL.pattern,
                   head, ok=(0, 1)).decode('utf-8', 'replace').splitlines()
        for hit in hits:
            line = hit.split(':', 3)[-1]
            if '/home/' in line or '/media/' in line:
                warnings.append(f'local path in {hit}')
            if any(not allowed_email(e) for e in EMAIL.findall(line)):
                warnings.append(f'e-mail address in {hit}')

    for warning in warnings:
        print(f'warning: {warning}')
    for finding in findings:
        print(f'FINDING: {finding}')
    print(f'summary: branches {", ".join(heads)}; tags {", ".join(tags) or "none"}; '
          f'{counts.get("commit", 0)} commits, {counts.get("blob", 0)} blobs, '
          f'{counts.get("tag", 0)} tag objects; {len(findings)} findings, {len(warnings)} warnings')
    return 1 if findings else 0


def main():
    commands = {'mapping': (mapping, 2), 'verify': (verify, 4)}
    command, arity = commands.get(sys.argv[1] if len(sys.argv) > 1 else None, (None, None))
    if command is None or len(sys.argv) != 2 + arity:
        sys.exit(__doc__)
    return command(*sys.argv[2:])


if __name__ == '__main__':
    sys.exit(main())
