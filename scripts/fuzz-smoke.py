import random, subprocess, sys, itertools, string

BIN = "./target/release/cull"  # run from repo root after `cargo build --release`
random.seed(1234)

def run(args, inp: bytes, timeout=10):
    try:
        p = subprocess.run([BIN]+args, input=inp, capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return ("TIMEOUT", args, inp[:80])
    if p.returncode == 101 or b"panicked" in p.stderr or b"RUST_BACKTRACE" in p.stderr:
        return ("PANIC", args, inp[:200], p.stderr[:300])
    return None

fails = []

# 1. random garbage HTML bytes
tag_soup = ['<div>', '</div>', '<a href="', '">', '<table>', '<tr>', '<td', '<!--', '-->',
            '<![CDATA[', ']]>', '<script>', '</script>', '<p', '=', '"', "'", '<', '>', '&',
            '&amp;', '&#x41;', '\x00', '\xff', 'é'.encode('latin-1').decode('latin-1'), 'text ',
            '<meta charset="', '<html>', '<b><i>', '</b>', '<br/>', '<svg><path d="M0 0"/></svg>']
for i in range(300):
    n = random.randint(0, 60)
    html = ''.join(random.choice(tag_soup) for _ in range(n)).encode('utf-8', 'replace')
    if random.random() < 0.3:
        html = bytes(random.randint(0,255) for _ in range(random.randint(0,300)))
    mode = random.choice([['div','-t'], ['a','-a','href'], ['--table'], ['--md'],
                          ['div','-j','{x: a @href, y: [li]}'], ['*'], ['div','-1','-t']])
    r = run(mode, html)
    if r: fails.append(r)

# 2. random templates
tmpl_bits = ['{', '}', '[', ']', ':', ',', 'a', '.x', '@href', '|', 'num', 'h2', ' ', '"q"',
             '#id', '>', '+', '~', '(', ')', 'nth-child(2)', '|num', '@', '|||', '::', '{}']
html = b'<div class="post"><h2>T</h2><a href="/u">l</a><span class="tag">t1</span></div>'
for i in range(400):
    n = random.randint(0, 25)
    t = ''.join(random.choice(tmpl_bits) for _ in range(n))
    r = run(['.post', '-j', t], html)
    if r: fails.append(r)

# 3. random selectors
sel_bits = ['div', '.', '#', '[', ']', '=', '"a"', ':', ':not(', ')', '*', '>', '~', '+', ' ',
            'a', ':nth-child(', '2n+1', 'has(', '::before', ',', '|']
for i in range(300):
    n = random.randint(1, 15)
    s = ''.join(random.choice(sel_bits) for _ in range(n))
    r = run([s, '-t'], html)
    if r: fails.append(r)

# 4. pathological structures
deep = ('<div>'*5000 + 'x' + '</div>'*5000).encode()
wide = (b'<li>x</li>'*100000)
bigattr = ('<a href="' + 'A'*1000000 + '">x</a>').encode()
badnest = b'<b><i><u><p></b></i></u></p>'*2000
huge_table = ('<table>' + '<tr>' + '<td colspan="9999" rowspan="9999">x</td>' + '</tr></table>').encode()
for html2, mode in [(deep,['div','-t']), (wide,['li','-t']), (bigattr,['a','-a','href']),
                    (badnest,['b','--md']), (huge_table,['--table']),
                    (deep,['--md']), (b'', ['div','-t']), (b'\xef\xbb\xbf', ['--table'])]:
    r = run(mode, html2, timeout=30)
    if r: fails.append(r)


# 5. XML: random soup through auto-detect, --xml, and RSS-ish fragments
xml_bits = ['<?xml version="1.0"?>', '<?xml encoding="', '"?>', '<rss>', '</rss>', '<channel>',
            '<item>', '</item>', '<link>', '</link>', '<media:thumbnail url="x"/>',
            '<![CDATA[', ']]>', '<!DOCTYPE x>', '<!-- c -->', '<a:b:c>', '</a:b:c>',
            '<x xmlns:m="u">', '</x>', '&lt;', '&#xZZ;', '&bogus;', '<?pi data?>',
            '<feed xmlns="http://www.w3.org/2005/Atom">', '</feed>', '<entry/>', '\x0c', ']]>']
xml_modes = [['item','-t'], ['link','-t'], ['*','--xml'], ['channel','-j','{t: title, l: link}'],
             ['item','--json-nodes'], ['rss','--html','-t'], ['--md'], ['item','-c']]
for i in range(300):
    n = random.randint(0, 50)
    doc = ''.join(random.choice(xml_bits) for _ in range(n)).encode('utf-8', 'replace')
    if random.random() < 0.2:
        doc = b'<?xml version="1.0" encoding="' + bytes(random.randint(32,126) for _ in range(random.randint(0,20))) + b'"?>' + doc
    r = run(random.choice(xml_modes), doc)
    if r: fails.append(r)

# 6. nested -j templates + filter chains (v0.8.0 surface)
nest_bits = ['{', '}', '[', ']', ':', ',', 'a', '.post', '@href', '|trim', '|lower', '|upper',
             '|num', 'h2', ' ', '"k"', '{t: h2}', '[.tag {n: span}]', 'div {', '} ', '|',
             'span', '@', '::', '{a: b {c: d}}', '[', ']]']
html_nest = (b'<div class="post"><h2> T </h2><a href="/u">l</a>'
             b'<span class="tag">t1</span><span class="tag">t2</span></div>')
for i in range(400):
    n = random.randint(0, 22)
    t = ''.join(random.choice(nest_bits) for _ in range(n))
    r = run(['.post', '-j', t], html_nest)
    if r: fails.append(r)

# 7. --json-nodes / new flags on garbage bytes
new_modes = [['div','--json-nodes'], ['div','--json-nodes','--array','-p'],
             ['*','-r','script','-t'], ['div','--has-text','x','-t'], ['div','-i'],
             ['div','-c'], ['div','-l'], ['p','--has-text','','--md']]
for i in range(200):
    doc = bytes(random.randint(0,255) for _ in range(random.randint(0,400)))
    if random.random() < 0.5:
        doc = ''.join(random.choice(tag_soup) for _ in range(random.randint(0,60))).encode('utf-8','replace')
    r = run(random.choice(new_modes), doc)
    if r: fails.append(r)

# 8. XML pathological
xml_deep = b'<?xml version="1.0"?>' + b'<a>'*5000 + b'x' + b'</a>'*5000
xml_unclosed = b'<rss><channel><item><title>x'
xml_badns = b'<x:y:z:w xmlns:'*100
huge_cdata = b'<?xml version="1.0"?><r><![CDATA[' + b'A'*1000000 + b']]></r>'
for doc, mode in [(xml_deep,['a','-t']), (xml_deep,['--md']), (xml_unclosed,['title','-t']),
                  (xml_badns,['*','--xml']), (huge_cdata,['r','-t']),
                  (b'<?xml?>',['*','-t']), (b'<?xml version="1.0"?>',['*','--json-nodes'])]:
    r = run(mode, doc, timeout=30)
    if r: fails.append(r)

print(f"failures: {len(fails)}")
for f in fails[:15]:
    print(f)
