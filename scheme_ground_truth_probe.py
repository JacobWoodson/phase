import json

db = json.load(open('D:/Code/phase/client/public/card-data.json', encoding='utf-8'))

# All scheme cards: type line contains 'Scheme'
schemes = {}
for key, e in db.items():
    ct = e.get('card_type') or {}
    tl = ' '.join(ct.get('supertypes', []) + ct.get('core_types', []) + ct.get('subtypes', []))
    if 'Scheme' in tl:
        schemes[e.get('name') or key] = e
print('TOTAL SCHEMES:', len(schemes))

def walk(node, path, hits):
    if isinstance(node, dict):
        t = node.get('type')
        if t == 'Unimplemented':
            hits.append((path, 'Unimpl', str(node.get('name')) + ' :: ' + str(node.get('description'))[:150]))
        for k, v in node.items():
            if k == 'Unknown':
                hits.append((path + '/' + k, 'Unknown', str(v)[:150]))
            walk(v, path + '/' + k, hits)
    elif isinstance(node, list):
        for i, v in enumerate(node):
            walk(v, f'{path}[{i}]', hits)

clean, gapped = [], []
for n in sorted(schemes):
    e = schemes[n]
    hits = []
    for f in ('abilities', 'triggers', 'static_abilities', 'replacements', 'keywords'):
        walk(e.get(f), f, hits)
    # dedupe bare-string double counts (walk already avoids those now)
    (clean if not hits else gapped).append((n, hits))

print(f'CLEAN: {len(clean)}  GAPPED: {len(gapped)}')
print('--- CLEAN ---')
for n, _ in clean:
    print(' ', n)
print('--- GAPPED ---')
for n, hits in gapped:
    print('=', n, f'({len(hits)})')
    for p, kind, detail in hits:
        print(f'   {kind}@{p}: {detail}')
