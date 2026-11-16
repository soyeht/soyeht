"""The cases of voice-turns.json, written by hand from the rules of docs/contracts/20-voice-loop.md.

`voice_vectors.py` lays them out. The cases are written for the values of `x-clientPolicy.voice`.
"""

import json

def a(do, at, **kw):
    d = {"do": do, "at": at}; d.update(kw); return d

def step(inp, at, phase, actions=None, **kw):
    d = {"input": inp, "at": at}; d.update(kw)
    d["actions"] = actions or []
    d["phase"] = phase
    return d

def frames(at, count, p, phase, actions=None):
    return step("frames", at, phase, actions, count=count, p=p)

def start(at=0): return step("start", at, "waiting")

def prelude():
    """Speech from 32 (noticed at 96), then silence until the model is asked at 448."""
    return [
        start(),
        frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
        frames(128, 11, 0.1, "deciding", [a("checkEndOfTurn", 448, id=1)]),
    ]

def finalizing():
    return prelude() + [step("endOfTurn", 450, "finalizing", [a("finalizeTurn", 450)], id=1, complete=True)]

def submitted(text="what time is it"):
    return finalizing() + [
        step("transcript", 600, "finalizing", text="what time", final=False),
        step("transcript", 700, "waitingReply", [a("submit", 700, text=text, interrupted=False)],
             text="  " + text + "  ", final=True),
    ]

def probingWait():
    return submitted() + [frames(800, 11, 0.9, "probingWait", [a("probeInterruption", 1120, fromMs=448)])]

def playing(sentence="It is half past three."):
    return submitted() + [step("sentenceStarted", 2000, "playing", text=sentence)]

def probing(sentence="It is half past three."):
    return playing(sentence) + [frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)])]

cases = []
def case(name, steps, config=None, reach=None):
    c = {"name": name}
    if config: c["config"] = config
    if reach: c["reach"] = reach
    c["steps"] = steps
    cases.append(c)

# ---- turns
case("a turn from the first word to the agent", submitted() + [
    step("sentenceStarted", 3000, "playing", text="It is half past three."),
    step("replyFinished", 5000, "waiting"),
])
case("voice before the loop starts is ignored", [
    frames(32, 5, 0.9, "off"), start(200), frames(232, 2, 0.9, "waiting"),
])
case("a burst shorter than the start time is not a turn", [
    start(), frames(32, 2, 0.9, "waiting"), frames(96, 20, 0.1, "waiting"),
])
case("a run of exactly the start time is a turn", [
    start(), frames(32, 3, 0.5, "hearing", [a("startTurn", 96, fromMs=0)]),
])
case("a probability just under the start threshold is not speech", [
    start(), frames(32, 20, 0.49, "waiting"),
])
case("a frame between the two thresholds keeps speech going and does not start it", [
    start(),
    frames(32, 1, 0.4, "waiting"),
    frames(64, 3, 0.6, "hearing", [a("startTurn", 128, fromMs=0)]),
    frames(160, 4, 0.4, "hearing"),
    frames(288, 11, 0.3, "deciding", [a("checkEndOfTurn", 608, id=1)]),
])
case("exactly the end threshold keeps speech going", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    frames(128, 5, 0.35, "hearing"),
    frames(288, 11, 0.1, "deciding", [a("checkEndOfTurn", 608, id=1)]),
])
case("just under the end threshold is silence", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    frames(128, 16, 0.34, "deciding", [a("checkEndOfTurn", 448, id=1)]),
])
case("the turn starts with audio from before the first voiced frame", [
    start(), frames(32, 40, 0.1, "waiting"),
    frames(1312, 3, 0.9, "hearing", [a("startTurn", 1376, fromMs=960)]),
])
case("the recognizer never starts from before the audio that is still kept", submitted() + [
    step("sentenceStarted", 2000, "playing", text="It is half past three."),
    frames(2048, 94, 0.9, "probing", [a("probeInterruption", 5024, fromMs=3024)]),
], {"bargeInMs": 3000, "probeTimeoutMs": 5000})
# silence and its equality
case("a pause of exactly the silence time asks the model", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    frames(128, 11, 0.1, "deciding", [a("checkEndOfTurn", 448, id=1)]),
], {"silenceMs": 352})
case("a pause a millisecond short of the silence time does not", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    frames(128, 11, 0.1, "hearing"),
    frames(480, 1, 0.1, "deciding", [a("checkEndOfTurn", 480, id=1)]),
], {"silenceMs": 353})
case("one voiced frame in ten does not hold a turn open", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    frames(128, 9, 0.1, "hearing"),
    frames(416, 1, 0.9, "hearing"),
    frames(448, 1, 0.1, "deciding", [a("checkEndOfTurn", 448, id=1)]),
    frames(480, 1, 0.9, "deciding"),
    frames(512, 1, 0.1, "deciding"),
])
case("speech that resumes during the check makes its verdict stale", prelude() + [
    frames(480, 3, 0.9, "hearing"),
    step("endOfTurn", 560, "hearing", id=1, complete=True),
    frames(576, 11, 0.1, "deciding", [a("checkEndOfTurn", 896, id=2)]),
])
case("a model that says the person is not done waits for more speech", prelude() + [
    step("endOfTurn", 460, "deciding", id=1, complete=False),
    frames(480, 30, 0.1, "deciding"),
    frames(1440, 23, 0.1, "finalizing", [a("finalizeTurn", 2112)]),
])
case("a long silence ends the turn although no verdict ever came", prelude() + [
    frames(480, 52, 0.1, "finalizing", [a("finalizeTurn", 2112)]),
])
case("a silence of exactly the long limit ends the turn", prelude() + [
    frames(480, 51, 0.1, "deciding"),
    frames(2112, 1, 0.1, "finalizing", [a("finalizeTurn", 2112)]),
], {"incompleteSilenceMs": 2016})
case("a silence a millisecond short of the long limit does not", prelude() + [
    frames(480, 52, 0.1, "deciding"),
    frames(2144, 1, 0.1, "finalizing", [a("finalizeTurn", 2144)]),
], {"incompleteSilenceMs": 2017})
case("a verdict that comes after the long silence is stale", prelude() + [
    frames(480, 52, 0.1, "finalizing", [a("finalizeTurn", 2112)]),
    step("endOfTurn", 2200, "finalizing", id=1, complete=True),
])
case("a verdict for another check is ignored", prelude() + [
    step("endOfTurn", 460, "deciding", id=7, complete=True),
])
case("a recognizer that heard nothing discards the turn", finalizing() + [
    step("transcript", 800, "waiting", [a("discardTurn", 800)], text="   ", final=True),
])
# finalizing time-out
case("the recognizer's last partial text is used when its final never comes", finalizing() + [
    step("transcript", 470, "finalizing", text="open the", final=False),
    step("transcript", 475, "finalizing", text="open the file", final=False),
    frames(480, 94, 0.1, "waitingReply",
           [a("discardTurn", 3456), a("submit", 3456, text="open the file", interrupted=False)]),
])
case("a turn the recognizer heard nothing of is dropped when its final never comes", finalizing() + [
    frames(480, 94, 0.1, "waiting", [a("discardTurn", 3456)]),
])
case("a wait of exactly the final-text limit ends it", finalizing() + [
    step("transcript", 470, "finalizing", text="open the file", final=False),
    frames(480, 94, 0.1, "waitingReply",
           [a("discardTurn", 3456), a("submit", 3456, text="open the file", interrupted=False)]),
], {"finalizeTimeoutMs": 3006})
case("a wait a millisecond short of the final-text limit does not", finalizing() + [
    step("transcript", 470, "finalizing", text="open the file", final=False),
    frames(480, 94, 0.1, "finalizing"),
    frames(3488, 1, 0.1, "waitingReply",
           [a("discardTurn", 3488), a("submit", 3488, text="open the file", interrupted=False)]),
], {"finalizeTimeoutMs": 3007})
# the recognizer ended before the turn did
case("the text of a recognizer that ended during the turn is used when the turn ends", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    step("transcript", 100, "hearing", text="open the", final=False),
    frames(128, 3, 0.1, "hearing"),
    step("transcript", 200, "hearing", text="open the file", final=True),
    frames(224, 8, 0.1, "deciding", [a("checkEndOfTurn", 448, id=1)]),
    step("endOfTurn", 450, "waitingReply", [a("submit", 450, text="open the file", interrupted=False)],
         id=1, complete=True),
])
case("the text of a recognizer that ended during the check is used at the long silence", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    frames(128, 11, 0.1, "deciding", [a("checkEndOfTurn", 448, id=1)]),
    step("transcript", 460, "deciding", text="open the file", final=True),
    frames(480, 52, 0.1, "waitingReply", [a("submit", 2112, text="open the file", interrupted=False)]),
])
case("a recognizer that ended with nothing leaves nothing to send", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    frames(128, 3, 0.1, "hearing"),
    step("transcript", 200, "hearing", text="", final=True),
    frames(224, 8, 0.1, "deciding", [a("checkEndOfTurn", 448, id=1)]),
    step("endOfTurn", 450, "waiting", [a("discardTurn", 450)], id=1, complete=True),
])
# thinking
case("talking while the agent thinks is listened to first, and words cancel its turn", submitted() + [
    frames(800, 11, 0.9, "probingWait", [a("probeInterruption", 1120, fromMs=448)]),
    step("transcript", 1150, "hearing", [a("cancelReply", 1150)], text="wait", final=False),
    frames(1152, 11, 0.1, "deciding", [a("checkEndOfTurn", 1472, id=2)]),
    step("endOfTurn", 1500, "finalizing", [a("finalizeTurn", 1500)], id=2, complete=True),
    step("transcript", 1600, "waitingReply",
         [a("submit", 1600, text="wait, one more thing", interrupted=False)],
         text="wait, one more thing", final=True),
])
case("a cough while the agent thinks does not cancel it", submitted() + [
    frames(800, 3, 0.9, "waitingReply"), frames(896, 20, 0.1, "waitingReply"),
])
case("a listener's noise while the agent thinks is dropped at the end of the pause", submitted() + [
    frames(800, 11, 0.9, "probingWait", [a("probeInterruption", 1120, fromMs=448)]),
    step("transcript", 1130, "probingWait", text="mhm", final=False),
    frames(1152, 11, 0.1, "probingWait", [a("finishProbe", 1472)]),
    step("transcript", 1500, "waitingReply", [a("dropProbe", 1500)], text="mhm", final=True),
])
case("a talk over the wait that decides nothing is dropped", submitted() + [
    frames(800, 58, 0.9, "waitingReply",
           [a("probeInterruption", 1120, fromMs=448), a("dropProbe", 2624)]),
])
case("a talk over the wait that went on is not listened to again until a pause", submitted() + [
    frames(800, 58, 0.9, "waitingReply",
           [a("probeInterruption", 1120, fromMs=448), a("dropProbe", 2624)]),
    frames(2656, 10, 0.9, "waitingReply"),
    frames(2976, 1, 0.1, "waitingReply"),
    frames(3008, 11, 0.9, "probingWait", [a("probeInterruption", 3328, fromMs=2656)]),
])
case("a reply starting while the person is thinking aloud is held until the probe ends", probingWait() + [
    step("sentenceStarted", 1200, "probingWait", text="It is half past three."),
])
# talking over the reply
case("talking over the reply with words takes the turn", probing() + [
    frames(2400, 4, 0.9, "probing"),
    step("transcript", 2500, "hearing",
         [a("stopPlayback", 2500), a("cancelReply", 2500)], text="wait", final=False),
    frames(2528, 11, 0.1, "deciding", [a("checkEndOfTurn", 2848, id=2)]),
    step("endOfTurn", 2900, "finalizing", [a("finalizeTurn", 2900)], id=2, complete=True),
    step("transcript", 3200, "waitingReply",
         [a("submit", 3200, text="wait, I meant the time in Tokyo", interrupted=True)],
         text="wait, I meant the time in Tokyo", final=True),
])
case("the probe's recognizer is finished at the pause, and its final text decides", probing() + [
    frames(2400, 11, 0.1, "probing", [a("finishProbe", 2720)]),
    step("transcript", 2800, "hearing", [a("stopPlayback", 2800), a("cancelReply", 2800)],
         text="para", final=True),
    frames(2816, 1, 0.1, "deciding", [a("checkEndOfTurn", 2816, id=2)]),
    step("endOfTurn", 2830, "waitingReply", [a("submit", 2830, text="para", interrupted=True)],
         id=2, complete=True),
])
case("speech after the final text of the probe gets a recognizer of its own and the words are kept", probing() + [
    frames(2400, 11, 0.1, "probing", [a("finishProbe", 2720)]),
    step("transcript", 2800, "hearing", [a("stopPlayback", 2800), a("cancelReply", 2800)],
         text="para", final=True),
    frames(2816, 3, 0.9, "hearing", [a("checkEndOfTurn", 2816, id=2), a("startTurn", 2880, fromMs=2800)]),
    step("transcript", 2900, "hearing", text="again please", final=False),
    frames(2912, 11, 0.1, "deciding", [a("checkEndOfTurn", 3232, id=3)]),
    step("endOfTurn", 3250, "finalizing", [a("finalizeTurn", 3250)], id=3, complete=True),
    step("transcript", 3300, "waitingReply",
         [a("submit", 3300, text="para again please", interrupted=True)],
         text="again please", final=True),
])
case("a recognizer that ended early gets a new one when the person speaks again, from after where it stopped", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    step("transcript", 100, "hearing", text="open the", final=False),
    frames(128, 3, 0.1, "hearing"),
    step("transcript", 200, "hearing", text="open the", final=True),
    frames(224, 1, 0.1, "hearing"),
    frames(256, 3, 0.9, "hearing", [a("startTurn", 320, fromMs=200)]),
    step("transcript", 340, "hearing", text="file please", final=False),
    frames(352, 11, 0.1, "deciding", [a("checkEndOfTurn", 672, id=1)]),
    step("endOfTurn", 680, "finalizing", [a("finalizeTurn", 680)], id=1, complete=True),
    step("transcript", 700, "waitingReply",
         [a("submit", 700, text="open the file please", interrupted=False)],
         text="file please", final=True),
])
case("a recognizer that ended early while the person goes on talking gets a new one at once", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    step("transcript", 100, "hearing", text="open the", final=False),
    frames(128, 10, 0.9, "hearing"),
    step("transcript", 420, "hearing", [a("startTurn", 420, fromMs=420)], text="open the", final=True),
    frames(448, 11, 0.1, "deciding", [a("checkEndOfTurn", 768, id=1)]),
    step("endOfTurn", 780, "finalizing", [a("finalizeTurn", 780)], id=1, complete=True),
    step("transcript", 800, "waitingReply",
         [a("submit", 800, text="open the file", interrupted=False)], text="file", final=True),
])
case("a word the sentence holds, or one that begins one, waits for the final text", playing("I do not know three things.") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="No", final=False),
    step("transcript", 2390, "probing", text="th", final=False),
    step("transcript", 2400, "hearing", [a("stopPlayback", 2400), a("cancelReply", 2400)],
         text="stop", final=False),
])
case("a listener's noise is recognised by whole words: the start of one waits, a start of a word does not", playing() + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="mm", final=False),
    step("transcript", 2390, "probing", text="Mm hmm", final=False),
    step("transcript", 2400, "hearing", [a("stopPlayback", 2400), a("cancelReply", 2400)],
         text="aha", final=False),
])
case("the probe's final text that is the avatar's own voice drops the probe", probing() + [
    frames(2400, 11, 0.1, "probing", [a("finishProbe", 2720)]),
    step("transcript", 2800, "playing", [a("dropProbe", 2800)], text="it is half past", final=True),
])
case("the probe's final text that is a listener's noise drops the probe", probing() + [
    frames(2400, 11, 0.1, "probing", [a("finishProbe", 2720)]),
    step("transcript", 2800, "playing", [a("dropProbe", 2800)], text="Uh-huh.", final=True),
])
case("a probe whose recognizer never answers is dropped at the limit", probing() + [
    frames(2400, 47, 0.1, "playing", [a("finishProbe", 2720), a("dropProbe", 3872)]),
])
case("a probe that ends at exactly the limit", probing() + [
    frames(2400, 46, 0.1, "probing", [a("finishProbe", 2720)]),
    frames(3872, 1, 0.1, "playing", [a("dropProbe", 3872)]),
], {"probeTimeoutMs": 1504})
case("a probe a millisecond short of the limit goes on", probing() + [
    frames(2400, 47, 0.1, "probing", [a("finishProbe", 2720)]),
    frames(3904, 1, 0.1, "playing", [a("dropProbe", 3904)]),
], {"probeTimeoutMs": 1505})
case("a talk of exactly the barge-in time is listened to", playing() + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
], {"bargeInMs": 352})
case("a talk a millisecond short of the barge-in time is not", playing() + [
    frames(2048, 11, 0.9, "playing"),
    frames(2400, 1, 0.9, "probing", [a("probeInterruption", 2400, fromMs=1696)]),
], {"bargeInMs": 353})
case("a short burst over the reply is not worth listening to", playing() + [
    frames(2048, 5, 0.9, "playing"), frames(2208, 20, 0.1, "playing"),
])
case("a probe that decides nothing is dropped, and the next waits for a pause", playing() + [
    frames(2048, 58, 0.9, "playing",
           [a("probeInterruption", 2368, fromMs=1696), a("dropProbe", 3872)]),
    frames(3904, 5, 0.9, "playing"),
    frames(4064, 1, 0.1, "playing"),
    frames(4096, 11, 0.9, "probing", [a("probeInterruption", 4416, fromMs=3744)]),
])
case("the reply ending during a probe drops it", probing() + [
    step("replyFinished", 2400, "waiting", [a("dropProbe", 2400)]),
])
# echo, noises, text
echo_a = "It is half past three."
case("the avatar's own voice heard by the microphone is not an interruption", playing() + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2370, "probing", text="it", final=False),
    step("transcript", 2380, "probing", text="it is half past", final=False),
    step("transcript", 2390, "probing", text="It is half past thr", final=False),
    frames(2400, 11, 0.1, "probing", [a("finishProbe", 2720)]),
    step("transcript", 2800, "playing", [a("dropProbe", 2800)], text="it is half past three", final=True),
])
case("an echo that crosses a sentence boundary is still an echo", playing("It is half past three.") + [
    step("sentenceStarted", 2010, "playing", text="Then we can leave."),
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="past three then we can", final=False),
])
case("one word that the sentence holds decides nothing as a partial text and takes the turn as the final", playing("I do not know the answer.") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="Not", final=False),
    frames(2400, 11, 0.1, "probing", [a("finishProbe", 2720)]),
    step("transcript", 2800, "hearing", [a("stopPlayback", 2800), a("cancelReply", 2800)],
         text="Not", final=True),
])
case("a word the sentence does not hold takes the turn at once", playing() + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "hearing", [a("stopPlayback", 2380), a("cancelReply", 2380)],
         text="Stop", final=False),
])
case("a word of the sentence said alone in another language waits for the pause too", playing("Vamos para casa agora.") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="Para!", final=False),
    frames(2400, 11, 0.1, "probing", [a("finishProbe", 2720)]),
    step("transcript", 2800, "hearing", [a("stopPlayback", 2800), a("cancelReply", 2800)],
         text="Para!", final=True),
])
case("a number said in words is not taken for the number written: a known limit", playing("It is 3:30 now.") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "hearing", [a("stopPlayback", 2380), a("cancelReply", 2380)],
         text="it is three thirty", final=False),
])
case("an echo in a script with no blanks matches by character", playing("今日はとても晴れです。") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="とても晴れ", final=False),
])
case("one character of such a script waits for the pause too", playing("今日はとても晴れです。") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="晴", final=False),
    frames(2400, 11, 0.1, "probing", [a("finishProbe", 2720)]),
    step("transcript", 2800, "hearing", [a("stopPlayback", 2800), a("cancelReply", 2800)],
         text="晴", final=True),
])
case("accents are compared composed: a decomposed echo is an echo", playing("Não sei o que fazer agora.") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="Não sei o que", final=False),
])
case("an apostrophe breaks a word on both sides", playing("I don't know what to do.") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="don't know what", final=False),
])
case("a listener's noises do not take the turn", playing() + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="uh", final=False),
    step("transcript", 2390, "probing", text="Uh-huh", final=False),
    step("transcript", 2395, "probing", text="aham, uhum", final=False),
])
case("an answer is not a listener's noise: yes takes the turn", playing("Do you want me to go on?") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "hearing", [a("stopPlayback", 2380), a("cancelReply", 2380)],
         text="Yes.", final=False),
])
case("an answer in Portuguese takes the turn too", playing("Quer que eu continue?") + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "hearing", [a("stopPlayback", 2380), a("cancelReply", 2380)],
         text="Sim", final=False),
])
case("a listener's noise followed by words takes the turn", playing() + [
    frames(2048, 11, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("transcript", 2380, "probing", text="mhm", final=False),
    step("transcript", 2450, "hearing",
         [a("stopPlayback", 2450), a("cancelReply", 2450)], text="mhm wait a second", final=False),
])
case("an empty transcript decides nothing", probing() + [
    step("transcript", 2380, "probing", text="  ", final=False),
])
case("words of a sentence played before the last two are not an echo", submitted() + [
    step("sentenceStarted", 2000, "playing", text="One two three."),
    step("sentenceStarted", 2500, "playing", text="Four five six."),
    step("sentenceStarted", 3000, "playing", text="Seven eight nine."),
    frames(3040, 11, 0.9, "probing", [a("probeInterruption", 3360, fromMs=2688)]),
    step("transcript", 3400, "probing", text="four five", final=False),
    step("transcript", 3405, "probing", text="six seven eight", final=False),
    step("transcript", 3410, "hearing",
         [a("stopPlayback", 3410), a("cancelReply", 3410)], text="one two", final=False),
])
# interrupts, stop, restart
case("an interrupt while thinking cancels the reply", submitted() + [
    step("interrupt", 1000, "waiting", [a("cancelReply", 1000)]),
])
case("an interrupt while hearing discards the turn", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    step("interrupt", 200, "waiting", [a("discardTurn", 200)]),
])
case("an interrupt while the avatar speaks stops it and lets the person talk", probing() + [
    step("interrupt", 2400, "waiting",
         [a("dropProbe", 2400), a("stopPlayback", 2400), a("cancelReply", 2400)]),
    frames(2432, 3, 0.9, "hearing", [a("startTurn", 2496, fromMs=2080)]),
])
case("stopping the loop stops what plays and leaves the agent to its work", playing() + [
    step("stop", 2100, "off", [a("stopPlayback", 2100)]),
    frames(2112, 5, 0.9, "off"),
])
case("stopping the loop while the agent thinks leaves it working", submitted() + [
    step("stop", 1000, "off"),
])
case("stopping the loop while a talk is listened to drops the listening and keeps the agent", probingWait() + [
    step("stop", 1200, "off", [a("dropProbe", 1200)]),
])
case("stopping while a turn is heard discards it", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    step("stop", 150, "off", [a("discardTurn", 150)]),
])
case("starting a loop that runs does nothing", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    step("start", 150, "hearing"),
])
case("the check numbers go on growing across a stop and a start", prelude() + [
    step("stop", 500, "off", [a("discardTurn", 500)]),
    start(600),
    frames(632, 3, 0.9, "hearing", [a("startTurn", 696, fromMs=280)]),
    frames(728, 11, 0.1, "deciding", [a("checkEndOfTurn", 1048, id=2)]),
    step("endOfTurn", 1100, "deciding", id=1, complete=True),
    step("endOfTurn", 1110, "finalizing", [a("finalizeTurn", 1110)], id=2, complete=True),
])
# replies without a voice turn
case("a reply that no voice turn asked for is spoken", [
    start(),
    step("sentenceStarted", 500, "playing", text="Good morning."),
    step("replyFinished", 900, "waiting"),
])
case("a reply that begins while the person talks waits", [
    start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)]),
    step("sentenceStarted", 150, "hearing", text="Good morning."),
])
# half duplex
half = {"halfDuplex": True}
case("half duplex: the microphone does not interrupt, a key does, and the speaker's tail is not heard after it", submitted() + [
    step("sentenceStarted", 2000, "playing", text="It is half past three."),
    frames(2048, 30, 0.9, "playing"),
    step("interrupt", 4000, "waiting", [a("stopPlayback", 4000), a("cancelReply", 4000)]),
    frames(4032, 9, 0.9, "waiting"),
    frames(4320, 6, 0.9, "hearing", [a("startTurn", 4384, fromMs=4300)]),
], half)
case("half duplex: the tail of the speaker does not start a turn, nor is recorded", submitted() + [
    step("sentenceStarted", 2000, "playing", text="It is half past three."),
    frames(2048, 30, 0.9, "playing"),
    step("replyFinished", 3000, "waiting"),
    frames(3008, 9, 0.9, "waiting"),
    frames(3296, 6, 0.9, "hearing", [a("startTurn", 3392, fromMs=3300)]),
], half)
case("full duplex: speech that goes on past the end of the reply is heard at once", submitted() + [
    step("sentenceStarted", 2000, "playing", text="It is half past three."),
    frames(2048, 30, 0.9, "probing", [a("probeInterruption", 2368, fromMs=1696)]),
    step("replyFinished", 3000, "waiting", [a("dropProbe", 3000)]),
    frames(3008, 1, 0.9, "hearing", [a("startTurn", 3008, fromMs=1696)]),
])

# ---- every phase against every input
reach = {
    "off": ([], 0),
    "waiting": ([start()], 0),
    "hearing": ([start(), frames(32, 3, 0.9, "hearing", [a("startTurn", 96, fromMs=0)])], 96),
    "deciding": (prelude(), 448),
    "finalizing": (finalizing(), 450),
    "waitingReply": (submitted(), 700),
    "probingWait": (probingWait(), 1120),
    "playing": (playing(), 2000),
    "probing": (probing(), 2368),
}
order = list(reach)
INTERRUPT = {"off": [], "waiting": [], "hearing": ["discardTurn"], "deciding": ["discardTurn"],
             "finalizing": ["discardTurn"], "waitingReply": ["cancelReply"],
             "probingWait": ["dropProbe", "cancelReply"], "playing": ["stopPlayback", "cancelReply"],
             "probing": ["dropProbe", "stopPlayback", "cancelReply"]}
STOP = {"off": [], "waiting": [], "hearing": ["discardTurn"], "deciding": ["discardTurn"],
        "finalizing": ["discardTurn"], "waitingReply": [],
        "probingWait": ["dropProbe"], "playing": ["stopPlayback"], "probing": ["dropProbe", "stopPlayback"]}
pairs = []
def pair(phase, name, inp, expected_actions, expected_phase, **kw):
    t = reach[phase][1]
    at = t + 50
    acts = [a(d, at, **extra) for d, extra in expected_actions]
    s = step(inp, at, expected_phase, acts, **kw)
    pairs.append({"name": f"{phase}: {name}", "reach": phase, "steps": [s]})
for p in order:
    pair(p, "start", "start", [], "waiting" if p == "off" else p)
    pair(p, "stop", "stop", [(d, {}) for d in STOP[p]], "off")
    pair(p, "interrupt", "interrupt",
         [(d, {}) for d in INTERRUPT[p]] if p not in ("off", "waiting") else [],
         p if p in ("off", "waiting") else "waiting")
for p in order:
    t = reach[p][1]
    for kind, prob in (("an unvoiced frame", 0.1), ("a voiced frame", 0.9)):
        s = step("frames", t + 32, p, [], count=1, p=prob)
        pairs.append({"name": f"{p}: {kind}", "reach": p, "steps": [s]})
    # an answer to the check
    if p == "deciding":
        pair(p, "a verdict of finished", "endOfTurn", [("finalizeTurn", {})], "finalizing", id=1, complete=True)
        pair(p, "a verdict of not finished", "endOfTurn", [], p, id=1, complete=False)
    else:
        pair(p, "a verdict of finished", "endOfTurn", [], p, id=1, complete=True)
    # transcripts
    for final in (False, True):
        label = "a final text" if final else "a partial text"
        if p == "finalizing" and final:
            pair(p, label, "transcript", [("submit", {"text": "hello there", "interrupted": False})],
                 "waitingReply", text="hello there", final=True)
        elif p == "probing":
            pair(p, label, "transcript",
                 [("stopPlayback", {}), ("cancelReply", {})] + ([("startTurn", {"fromMs": reach[p][1] + 50})] if final else []),
                 "hearing", text="hello there", final=final)
        elif p == "probingWait":
            pair(p, label, "transcript",
                 [("cancelReply", {})] + ([("startTurn", {"fromMs": reach[p][1] + 50})] if final else []),
                 "hearing", text="hello there", final=final)
        elif p == "hearing" and final:
            pair(p, label, "transcript", [("startTurn", {"fromMs": reach[p][1] + 50})], p,
                 text="hello there", final=True)
        else:
            pair(p, label, "transcript", [], p, text="hello there", final=final)
    nxt = {"off": "off", "waiting": "playing", "waitingReply": "playing"}.get(p, p)
    pair(p, "a sentence begins", "sentenceStarted", [], nxt, text="Good morning.")
    fin = {"waitingReply": ([], "waiting"), "playing": ([], "waiting"),
           "probing": ([("dropProbe", {})], "waiting"), "probingWait": ([("dropProbe", {})], "waiting")}
    acts, ph = fin.get(p, ([], p))
    pair(p, "the replies end", "replyFinished", acts, ph)

out = {"policy": "x-clientPolicy.voice", "reach": {k: v[0] for k, v in reach.items()},
       "cases": cases, "pairs": pairs}

def dump_case(c, i, n, lines):
    lines.append('    {')
    lines.append('      "name": ' + json.dumps(c["name"], ensure_ascii=False) + ',')
    if "config" in c:
        lines.append('      "config": ' + json.dumps(c["config"]) + ',')
    if "reach" in c:
        lines.append('      "reach": ' + json.dumps(c["reach"]) + ',')
    lines.append('      "steps": [')
    for j, s in enumerate(c["steps"]):
        lines.append('        ' + json.dumps(s, ensure_ascii=False) + (',' if j < len(c["steps"]) - 1 else ''))
    lines.append('      ]')
    lines.append('    }' + (',' if i < n - 1 else ''))

lines = ['{', '  "policy": "x-clientPolicy.voice",']
lines.append('  "reach": {')
for i, (k, v) in enumerate(out["reach"].items()):
    lines.append(f'    {json.dumps(k)}: [')
    for j, s in enumerate(v):
        lines.append('      ' + json.dumps(s, ensure_ascii=False) + (',' if j < len(v) - 1 else ''))
    lines.append('    ]' + (',' if i < len(out["reach"]) - 1 else ''))
lines.append('  },')
lines.append('  "cases": [')
for i, c in enumerate(cases): dump_case(c, i, len(cases), lines)
lines.append('  ],')
lines.append('  "pairs": [')
for i, c in enumerate(pairs):
    lines.append('    ' + json.dumps(c, ensure_ascii=False) + (',' if i < len(pairs) - 1 else ''))
lines.append('  ]')
lines.append('}')
TEXT = '\n'.join(lines) + '\n'
