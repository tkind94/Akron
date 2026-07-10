"""Planted fixture (TKI-33): the F4 bug in miniature. `AlphaSuite`/`BetaSuite`
write byte-identical `__init__` and `run` methods (an exact-clone core for
each role); `GammaSuite` drifts both — reordered field assignments, one fewer
call — just enough to fall below theta_clone against its role's core (so it
stays out of the tight cluster) while clearing theta_family (so the family
altitude reunites it, same as the graded record-parser lineage). Both roles
also read as "rows of `self.x = ...` plus calls" to each other and share the
same field vocabulary (source/sink/batcher/tracker/limiter/notifier), so the
pre-TKI-33 Channel-B coherence gate alone would have let init and run
assemble into one family — exactly as three `__init__` methods glued onto
a suite-runner `run` family on corpus-R (a private grading corpus). Only the dunder role guard keeps every
`__init__` in its own family, separate from every `run`.
"""


class AlphaSuite:
    def __init__(self, source, sink, batcher, tracker, limiter, notifier):
        self.source = source
        self.sink = sink
        self.batcher = batcher
        self.tracker = tracker
        self.limiter = limiter
        self.notifier = notifier
        self.tracker.record(self.source)
        self.limiter.wait()
        self.sink.write(self.batcher)
        self.notifier.ping(self.source)
        self.state = "idle"
        self.attempts = 0

    def run(self):
        source, sink, batcher, tracker, limiter, notifier = (
            self.source, self.sink, self.batcher, self.tracker, self.limiter, self.notifier
        )
        self.source = source
        self.sink = sink
        self.batcher = batcher
        self.tracker = tracker
        self.limiter = limiter
        self.notifier = notifier
        tracker.record(source)
        limiter.wait()
        sink.write(batcher)
        notifier.ping(source)
        self.state = "done"
        return self.attempts


class BetaSuite:
    def __init__(self, source, sink, batcher, tracker, limiter, notifier):
        self.source = source
        self.sink = sink
        self.batcher = batcher
        self.tracker = tracker
        self.limiter = limiter
        self.notifier = notifier
        self.tracker.record(self.source)
        self.limiter.wait()
        self.sink.write(self.batcher)
        self.notifier.ping(self.source)
        self.state = "idle"
        self.attempts = 0

    def run(self):
        source, sink, batcher, tracker, limiter, notifier = (
            self.source, self.sink, self.batcher, self.tracker, self.limiter, self.notifier
        )
        self.source = source
        self.sink = sink
        self.batcher = batcher
        self.tracker = tracker
        self.limiter = limiter
        self.notifier = notifier
        tracker.record(source)
        limiter.wait()
        sink.write(batcher)
        notifier.ping(source)
        self.state = "done"
        return self.attempts


class GammaSuite:
    """The drifted third role pair: same fields, reordered assignments, one
    fewer call than Alpha/Beta's exact clones — enough drift to sit below
    theta_clone against the exact core, yet above theta_family.
    """

    def __init__(self, source, sink, batcher, tracker, limiter, notifier):
        self.notifier, self.limiter, self.tracker, self.batcher, self.sink, self.source = (
            notifier, limiter, tracker, batcher, sink, source
        )
        self.tracker.record(self.source)
        self.state = "idle"
        self.attempts = 0

    def run(self):
        source, sink, batcher, tracker, limiter, notifier = (
            self.source, self.sink, self.batcher, self.tracker, self.limiter, self.notifier
        )
        self.notifier, self.limiter, self.tracker, self.batcher, self.sink, self.source = (
            notifier, limiter, tracker, batcher, sink, source
        )
        tracker.record(source)
        self.state = "done"
        return self.attempts
