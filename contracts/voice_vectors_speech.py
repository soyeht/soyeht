"""The cases of speech-text.json, written by hand from the rules of docs/contracts/20-voice-loop.md.

`voice_vectors.py` lays them out. The cases are written for the values of `x-clientPolicy.voice`.
"""

import json

CH = {}
speech = []
def sp(name, md, spoken, streaming=False):
    d = {"name": name, "markdown": md}
    if streaming: d["streaming"] = True
    d["spoken"] = spoken
    speech.append(d)

sp("plain text", "Hello world", "Hello world")
sp("emphasis is read as its words and struck text is not", "a **b** *c* ~~d~~ e", "a b c e")
sp("inline code is read as words", "Run `make test` now", "Run make test now")
sp("a link is read as its label", "See [the docs](https://example.com/docs) for more", "See the docs for more")
sp("a bare web address is dropped", "Open https://example.com/x now", "Open now")
sp("a bare web address keeps the stop of its sentence", "Open https://example.com/x. Then go.", "Open. Then go.")
sp("an address in brackets is dropped", "Open (https://example.com/x) now", "Open now")
sp("an address with www is dropped", "Try www.example.com today", "Try today")
sp("an image is read as its description", "![a chart of sales](https://example.com/c.png)", "a chart of sales")
sp("HTML tags are dropped", "Hello <b>world</b> and <br/>more", "Hello world and more")
sp("a code block is not read", "Here:\n\n```swift\nlet x = 1\n```\n\nDone.", "Here:\nDone.")
sp("an indented code block is not read", "Before\n\n    let x = 1\n\nAfter", "Before\nAfter")
sp("a heading and a paragraph are two lines", "# Title\n\nBody text.", "Title\nBody text.")
sp("a bullet list is one line per item", "- one\n- two\n- three", "one\ntwo\nthree")
sp("an ordered list is read with no numbers", "1. first\n2. second", "first\nsecond")
sp("a nested list is flattened", "- a\n  - b\n- c", "a\nb\nc")
sp("a quote is read as its words", "> quoted words\n\nafter", "quoted words\nafter")
sp("a table is not read", "| a | b |\n|---|---|\n| 1 | 2 |\n\nAfter", "After")
sp("a rule is not read", "one\n\n---\n\ntwo", "one\ntwo")
sp("emoji are dropped", "Great job \U0001F389 done ✅", "Great job done")
sp("a keycap, an hourglass and arrows are dropped", "Step 1️⃣ ⏳ A → B ⬆", "Step 1 A B")
sp("a line of only emoji is dropped", "\U0001F389\U0001F389\n\nText", "Text")
sp("a line of only symbols is dropped", "&&& ###\n\nText", "Text")
sp("a task list loses its markers", "- [x] done\n- [ ] todo", "done\ntodo")
sp("a line break inside a paragraph is a blank", "line one\nline two", "line one line two")
sp("blanks are collapsed", "a    b \t c", "a b c")
sp("nothing to say", "", "")
sp("only code", "```\nx\n```", "")
sp("numbers and accents are kept", "Olá, tudo bem? 3.5 por cento.", "Olá, tudo bem? 3.5 por cento.")
sp("an unfinished bold span is read as its words", "Here is **the", "Here is the", streaming=True)
sp("an unfinished link is held back", "See [the docs", "See", streaming=True)
sp("a link whose address is unfinished is held back", "See [the docs](https://exa", "See", streaming=True)
sp("a finished link is read", "See [the docs](https://example.com) now", "See the docs now", streaming=True)
sp("an unmatched star is held back", "It is *very", "It is", streaming=True)
sp("a matched star is read", "It is *very* good", "It is very good", streaming=True)
sp("a bullet star is not held back", "* one\n* two", "one\ntwo", streaming=True)
sp("a finished paragraph ends the text with a newline", "Done.\n\n", "Done.\n", streaming=True)
sp("a paragraph that may go on does not", "Done.\n", "Done.", streaming=True)
sp("a block after the last line finishes it", "Hello\n\n```\nx\n```", "Hello\n", streaming=True)
sp("an unmatched underscore is held back", "It is _very", "It is", streaming=True)
sp("a matched underscore is read", "It is _very_ good", "It is very good", streaming=True)
sp("an underscore inside a word is not emphasis", "the snake_case name", "the snake_case name", streaming=True)
sp("only the last line is looked at for an unfinished link", "see [a\nnext line", "see [a next line", streaming=True)
sp("a pair of stars is skipped when looking for a lone one", "It is **very** *good", "It is very", streaming=True)
sp("a backtick opens and closes code, where a bracket is not a link", "a `b[c` d [e", "a b[c d", streaming=True)
sp("a bracket not followed by an address is closed", "See [the docs] and [more", "See [the docs] and", streaming=True)
sp("nothing is held inside a code block", "```\nsee [x", "", streaming=True)

chunker = []
def ch(name, updates, config=None):
    d = {"name": name}
    if config: d["config"] = config
    d["updates"] = [{"spoken": s, "final": f, "emit": e} for s, f, e in updates]
    chunker.append(d)

ch("a sentence is spoken once a blank and the next word follow its stop", [
    ("The first sentence is here.", False, []),
    ("The first sentence is here. ", False, []),
    ("The first sentence is here. Then", False, ["The first sentence is here."]),
    ("The first sentence is here. Then more words.", False, []),
    ("The first sentence is here. Then more words.", True, ["Then more words."]),
])
ch("a short sentence is joined to the next", [
    ("Yes. I can do that for you. ", False, []),
    ("Yes. I can do that for you. Sure", False, ["Yes. I can do that for you."]),
])
ch("a short last sentence waits unless the reply is over", [
    ("Let me look at the code now. Ok. ", False, ["Let me look at the code now."]),
    ("Let me look at the code now. Ok. ", True, ["Ok."]),
])
ch("a finished line is spoken whole", [
    ("Title\nBody sentence goes here", False, ["Title"]),
    ("Title\nBody sentence goes here.", True, ["Body sentence goes here."]),
])
ch("lines are never joined", [
    ("One.\nTwo.\nThree and more words here.", True, ["One.", "Two.", "Three and more words here."]),
])
ch("blank lines are skipped", [("One.\n\nTwo.\n", True, ["One.", "Two."])])
ch("a full stop in a number or an abbreviation is not an end", [
    ("Pi is 3.14 and that is that. See e.g. the docs. ", False, ["Pi is 3.14 and that is that."]),
    ("Pi is 3.14 and that is that. See e.g. the docs. ", True, ["See e.g. the docs."]),
])
ch("a title before a name is not an end", [
    ("Please call Dr. Smith about it today. Thanks", False, ["Please call Dr. Smith about it today."]),
])
ch("a street abbreviation is not an end", [
    ("Go along Av. Paulista until the end. Then", False, ["Go along Av. Paulista until the end."]),
])
ch("a full stop before a word in lowercase is not an end", [
    ("We meet at 5 p.m. today and then go. ", False, []),
    ("We meet at 5 p.m. today and then go. Then", False, ["We meet at 5 p.m. today and then go."]),
    ("Apples, pears, etc. and more fruit. Next", False, ["Apples, pears, etc. and more fruit."]),
])
ch("a full stop before a word that starts a sentence is an end, even after an hour", [
    ("It ended at 5 p.m. Then we left.", False, ["It ended at 5 p.m."]),
])
ch("a closing quote belongs to its sentence", [
    ('He said "go home." Then he left. ', False, ['He said "go home."']),
])
ch("question marks, exclamation marks and an ellipsis end a sentence at a blank", [
    ("Really?! Yes… I think so? ", False, ["Really?! Yes… I think so?"]),
])
ch("a stop of a script with no blanks ends a sentence at once", [
    ("今日はとても晴れです。明日は雨です。", False, ["今日はとても晴れです。明日は雨です。"]),
])
ch("the same text twice gives nothing the second time", [
    ("Hello there, my friend. How", False, ["Hello there, my friend."]),
    ("Hello there, my friend. How", False, []),
    ("Hello there, my friend. How are you today? ", False, ["How are you today?"]),
])
ch("the end of the reply flushes an unfinished tail", [("Almost done", False, []), ("Almost done", True, ["Almost done"])])
ch("a long sentence is cut after a clause that has a blank after it, then at a blank", [
    ("Alpha beta gamma delta, epsilon zeta eta theta iota kappa lambda mu.", True,
     ["Alpha beta gamma delta,", "epsilon zeta eta theta iota kappa", "lambda mu."]),
], {"maximumLength": 40})
ch("a comma with no blank after it is not a place to cut", [
    ("Alpha,beta,gamma,delta,epsilon zeta eta theta iota kappa lambda mu.", True,
     ["Alpha,beta,gamma,delta,epsilon zeta eta", "theta iota kappa lambda mu."]),
], {"maximumLength": 40})
ch("a tail that outgrows the limit is spoken in parts before the reply is over", [
    ("Alpha beta gamma delta, epsilon zeta eta theta iota kappa lambda", False, ["Alpha beta gamma delta,"]),
    ("Alpha beta gamma delta, epsilon zeta eta theta iota kappa lambda mu. ", False,
     ["epsilon zeta eta theta iota kappa"]),
    ("Alpha beta gamma delta, epsilon zeta eta theta iota kappa lambda mu. ", True, ["lambda mu."]),
], {"maximumLength": 40})
ch("a text that is not an extension goes back to the last sentence both share", [
    ("Alpha beta gamma delta. Epsilon zeta eta theta iota. Omega", False,
     ["Alpha beta gamma delta.", "Epsilon zeta eta theta iota."]),
    ("Alpha beta gamma delta. Epsilon zeta changed words here. Omega", False,
     ["Epsilon zeta changed words here."]),
])
ch("a text that changes before anything was cut loses nothing", [
    ("Alpha beta gamm", False, []),
    ("Different words here. Then", False, ["Different words here."]),
])

stream = []
def st(name, updates):
    stream.append({"name": name, "updates": [{"markdown": m, "final": f, "emit": e} for m, f, e in updates]})

st("a reply spoken as it arrives, with a code block between", [
    ("Here is the plan.\n\n", False, ["Here is the plan."]),
    ("Here is the plan.\n\n```sh\nmake all\n```\n\nThen", False, []),
    ("Here is the plan.\n\n```sh\nmake all\n```\n\nThen we ship **it", False, []),
    ("Here is the plan.\n\n```sh\nmake all\n```\n\nThen we ship **it** now.", True, ["Then we ship it now."]),
])
st("a heading is spoken as a line of its own", [
    ("# Plan", False, []),
    ("# Plan\n\nFirst", False, ["Plan"]),
    ("# Plan\n\nFirst we look at the code.", True, ["First we look at the code."]),
])
st("a link is spoken once it is whole", [
    ("See [the docs", False, []),
    ("See [the docs](https://example.com/d) for more. Then", False, ["See the docs for more."]),
])
st("an italic is spoken once it is closed", [
    ("It is *very", False, []),
    ("It is *very* good. Then", False, ["It is very good."]),
])
st("an address at the end of a sentence leaves its stop", [
    ("See https://example.com. Then we go.", True, ["See. Then we go."]),
])

replies = []
def rp(name, steps):
    replies.append({"name": name, "steps": steps})
def up(id, md, final, emit, pending, accepted=True):
    return {"update": {"id": id, "markdown": md, "final": final}, "emit": emit, "accepted": accepted,
            "pending": pending}

rp("a reply is one message", [
    up("a", "Hello there, my friend. How", False, ["Hello there, my friend."], True),
    up("a", "Hello there, my friend. How are you today?", True, ["How are you today?"], False),
])
rp("another message is another reply, spoken after the first", [
    up("a", "Hello there, my friend. How", False, ["Hello there, my friend."], True),
    up("b", "Anything else you need? Just", False, ["Anything else you need?"], True),
    up("a", "Hello there, my friend. How are you?", True, ["How are you?"], True),
    up("b", "Anything else you need? Just say so.", True, ["Just say so."], False),
])
rp("a reply that no turn asked for is a reply like any other", [
    up("x", "Good morning, how are you?", True, ["Good morning, how are you?"], False),
])
rp("the late words of a cancelled reply are never spoken", [
    up("a", "Hello there, my friend. How", False, ["Hello there, my friend."], True),
    {"cancel": True, "pending": False},
    up("a", "Hello there, my friend. How are you today? Fine", False, [], False, accepted=False),
    up("b", "Here is the new answer. Done", False, ["Here is the new answer."], True),
])
rp("a message whose text changed is cut again from the last sentence it shares", [
    up("a", "Alpha beta gamma delta. Epsilon zeta eta theta iota. Omega", False,
       ["Alpha beta gamma delta.", "Epsilon zeta eta theta iota."], True),
    up("a", "Alpha beta gamma delta. Epsilon zeta changed words here. Omega", False,
       ["Epsilon zeta changed words here."], True),
])
rp("a reply that is only code ends with nothing to say", [
    up("a", "```\nmake test\n```", True, [], False),
])

heard = []
def hd(name, played, current, fraction, text, timings=None, position=None):
    d = {"name": name, "played": played, "current": current, "fraction": fraction}
    if timings is not None: d["timings"] = timings
    if position is not None: d["position"] = position
    d["heard"] = text
    heard.append(d)
hd("half of the sentence that was playing", ["One.", "Two."], "three four five six", 0.5, "One. Two. three four")
hd("none of it", ["One.", "Two."], "three four five six", 0, "One. Two.")
hd("all of it", ["One.", "Two."], "three four five six", 1, "One. Two. three four five six")
hd("whole words only", [], "hello there you", 0.34, "hello")
hd("no sentence was playing", ["A."], None, 0.5, "A.")
hd("a share above one is all", [], "a b", 1.7, "a b")
hd("a share below zero is none", ["A."], "a b", -1, "A.")
hd("nothing heard", [], None, 0, "")
hd("a script with no blanks is cut by character", [], "今日はとても晴れです", 0.5, "今日はとて")
hd("punctuation stays with the word it follows", [], "Hello, there! You", 0.7, "Hello, there!")
words = [{"text": "one", "start": 0.0}, {"text": "two", "start": 0.5}, {"text": "three.", "start": 1.0}]
hd("a word is heard once the next has begun, by the voice's own timings", [], "one two three.", 0.2, "one", words, 0.6)
hd("two words by the timings", [], "one two three.", 0.4, "one two", words, 1.2)
hd("the last word is heard when the whole sentence was", [], "one two three.", 1, "one two three.", words, 2.0)
hd("the timings win over the share", [], "one two three.", 0.9, "", words, 0.2)

def lines_of(items, indent=4):
    return ",\n".join(" " * indent + json.dumps(i, ensure_ascii=False) for i in items)

out = '{\n'
out += '  "policy": "x-clientPolicy.voice",\n'
out += '  "speechText": [\n' + lines_of(speech) + '\n  ],\n'
out += '  "chunker": [\n' + lines_of(chunker) + '\n  ],\n'
out += '  "stream": [\n' + lines_of(stream) + '\n  ],\n'
out += '  "replies": [\n' + lines_of(replies) + '\n  ],\n'
out += '  "heard": [\n' + lines_of(heard) + '\n  ]\n}\n'
TEXT = out
