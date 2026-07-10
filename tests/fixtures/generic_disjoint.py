"""Planted fixture (TKI-22): a generic-shape drifted variant whose VOCABULARY is
disjoint from the record-parser lineage. It shares the loop / guard / accumulate
skeleton of the parser family closely enough that its Channel-A cosine to the core
lands in the drift band (above theta_family — a merge candidate), yet it is a clone
of nothing (below theta_clone to every member, so it stays a singleton unit). Every
external name and identifier subword is audio-DSP domain, sharing no term with the
parser family. Channel A alone would pull it in; the Channel-B coherence gate must
keep it out. This is the scrapy blob in miniature: generic shape, disjoint vocab.
"""


def mix_channels(buffer):
    samples = []
    for frame in buffer.fetch("waveforms", []):
        gains = frame.fetch("amplitudes", {})
        if gains is None:
            break
        stereo = gains.fetch("left") + gains.fetch("right")
        samples.append(stereo)
    return samples
