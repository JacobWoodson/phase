import json

db = json.load(open('D:/Code/phase/client/public/card-data.json', encoding='utf-8'))
names = ["All in Good Time","Approach My Molten Realm","Choose Your Champion","Dance, Pathetic Marionette","I Bask in Your Silent Awe","I Know All, I See All","Into the Earthen Maw","My Crushing Masterstroke","My Undead Horde Awakens","My Wish Is Your Command","Nature Demands an Offering","Nature Shields Its Own","Nothing Can Stop Me Now","Only Blood Ends Your Nightmares","The Dead Shall Serve","The Fate of the Flammable","The Very Soil Shall Shake","Which of You Burns Brightest?","Because I Have Willed It","Bow to My Command","For Each of You, a Gift","Know Evil","My Forces Are Innumerable","My Laughter Echoes","Power Without Equal","When Will You Learn?"]

def walk(node, path, hits):
    if isinstance(node, dict):
        t = node.get('type')
        if t == 'Unimplemented' or (isinstance(node.get('effect'), str) and node.get('effect') == 'Unimplemented'):
            hits.append((path, 'Unimplemented', str(node.get('name')) + ' :: ' + str(node.get('description'))[:220]))
        for k, v in node.items():
            if k == 'Unknown' or v == 'Unknown':
                hits.append((path + '/' + k, 'Unknown', str(v)[:220]))
            walk(v, path + '/' + k, hits)
    elif isinstance(node, list):
        for i, v in enumerate(node):
            walk(v, f'{path}[{i}]', hits)
    elif isinstance(node, str) and node == 'Unimplemented':
        hits.append((path, 'Unimplemented', '(bare string)'))

for n in names:
    e = db.get(n.lower())
    hits = []
    for f in ('abilities', 'triggers', 'static_abilities', 'replacements', 'keywords'):
        walk(e.get(f), f, hits)
    print('=' * 100)
    print('###', n, f'({len(hits)} gap markers)')
    print('ORACLE:', (e.get('oracle_text') or '').replace('\n', ' | '))
    for p, kind, detail in hits:
        print(f'  {kind}@{p}: {detail}')
