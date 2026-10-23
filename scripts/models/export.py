"""Exports the two listening models to midzer's own weight format (.mzm), and
reference inputs and outputs that the Rust tests check the port against.

  - Beat This! small0 (Foscarin, Schlüter, Widmer, ISMIR 2024; MIT): beats and
    downbeats from a log-mel spectrogram. The JKU checkpoint server is often
    down, so the weights are read from the ONNX export in the Hugging Face repo
    ashudesai/songbird-models (`small0.onnx`). Its graph is broken, but its
    weights are the published ones: read the same way, its `final0.onnx`
    matches the `final0` checkpoint to 2e-5.
  - BTC, the bi-directional transformer for chords (Park et al., ISMIR 2019;
    MIT), large vocabulary (170 classes): `test/btc_model_large_voca.pt` in
    github.com/jayg996/BTC-ISMIR19.

Needs: torch, onnx, numpy, librosa, beat_this (pip install
git+https://github.com/CPJKU/beat_this) and a checkout of BTC-ISMIR19 (its
utils/hparams.py needs yaml.safe_load, and np.float → float for numpy 2).

  python scripts/models/export.py <small0.onnx> <BTC-ISMIR19 dir>

The .mzm format, little-endian: b"MZM1", u32 tensor count, then per tensor
u16 name length, the name (UTF-8), u8 dtype (0 = f16, 1 = f32), u8 rank,
u32 dims, and the data. Linear and attention weights are stored as
[in, out] (transposed from PyTorch); everything else keeps PyTorch's layout.
"""
import os, struct, sys
import numpy as np
import torch

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
MODELS = os.path.join(ROOT, "crates", "midzer-hear", "models")
TESTDATA = os.path.join(ROOT, "crates", "midzer-hear", "tests", "data")


def write_mzm(path, tensors, dtype=0):
    with open(path, "wb") as f:
        f.write(b"MZM1")
        f.write(struct.pack("<I", len(tensors)))
        for name, t in tensors.items():
            if isinstance(t, torch.Tensor):
                t = t.detach()
            a = np.ascontiguousarray(np.asarray(t, dtype=np.float32))
            b = name.encode()
            f.write(struct.pack("<H", len(b)) + b)
            f.write(struct.pack("<BB", dtype, a.ndim))
            for d in a.shape:
                f.write(struct.pack("<I", d))
            f.write(a.astype(np.float16 if dtype == 0 else np.float32).tobytes())
    print(f"wrote {os.path.relpath(path, ROOT)}: {os.path.getsize(path) / 1e6:.1f} MB")


def f16(t):
    """Rounds like the stored weights, so the references match the Rust port."""
    return torch.tensor(np.asarray(t, dtype=np.float32).astype(np.float16).astype(np.float32))


# ---------------------------------------------------------------- Beat This!

def beat_this_from_onnx(path):
    import onnx
    from onnx import numpy_helper
    from beat_this.model.beat_tracker import BeatThis

    m = onnx.load(path)
    init = {i.name: torch.tensor(numpy_helper.to_array(i).copy()) for i in m.graph.initializer}
    named = {k[len("model."):]: v for k, v in init.items() if k.startswith("model.")}
    for n in m.graph.node:
        if n.op_type not in ("MatMul", "Conv"):
            continue
        ws = [i for i in n.input if i in init and not i.startswith("model.")]
        if not ws:
            continue
        parts = []
        for p in n.name.strip("/").split("/")[1:-1]:
            if parts and "." in p and p.split(".")[0] == parts[-1]:
                parts[-1] = p
            else:
                parts.append(p)
        mod = ".".join(parts)
        if n.op_type == "MatMul":
            named[mod + ".weight"] = init[ws[0]].T.contiguous()
        else:
            # The export folded each conv's batch norm into it.
            named[mod + ".weight"] = init[ws[0]]
            named[mod + ".conv_bias"] = init[ws[1]]
    net = BeatThis(transformer_dim=128)
    sd = net.state_dict()
    new = {k: named[k].reshape(v.shape) for k, v in sd.items() if k in named}
    eps = 1e-5
    for conv, bn in [("frontend.stem.conv2d", "frontend.stem.bn2d")] + [(f"frontend.blocks.{i}.conv2d", f"frontend.blocks.{i}.norm") for i in range(3)]:
        n = sd[bn + ".weight"].shape[0]
        new[bn + ".weight"] = torch.full((n,), (1 + eps) ** 0.5)
        new[bn + ".bias"] = named[conv + ".conv_bias"]
        new[bn + ".running_mean"] = torch.zeros(n)
        new[bn + ".running_var"] = torch.ones(n)
    for k, v in sd.items():
        if k not in new:
            new[k] = v  # batch counters and the rotary frequencies
    net.load_state_dict(new, strict=True)
    return net.eval()


def export_beat_this(onnx_path):
    net = beat_this_from_onnx(onnx_path)
    sd = {k: v.detach() for k, v in net.state_dict().items()}
    out = {}
    # The input batch norm as a scale and offset per mel band.
    bn = net.frontend.stem.bn1d
    scale = bn.weight / torch.sqrt(bn.running_var + bn.eps)
    out["stem.scale"] = scale
    out["stem.offset"] = bn.bias - bn.running_mean * scale
    convs = [("stem", "frontend.stem.conv2d", "frontend.stem.bn2d")] + [(f"block{i}", f"frontend.blocks.{i}.conv2d", f"frontend.blocks.{i}.norm") for i in range(3)]
    for name, conv, norm in convs:
        b = getattr(net.frontend.stem, "bn2d") if name == "stem" else net.frontend.blocks[int(name[-1])].norm
        s = b.weight / torch.sqrt(b.running_var + b.eps)
        out[f"{name}.conv.weight"] = sd[conv + ".weight"] * s[:, None, None, None]
        out[f"{name}.conv.bias"] = b.bias - b.running_mean * s

    def attention(prefix, src):
        out[prefix + ".norm"] = sd[src + ".norm.gamma"]
        out[prefix + ".qkv"] = sd[src + ".to_qkv.weight"].T
        out[prefix + ".gates.weight"] = sd[src + ".to_gates.weight"].T
        out[prefix + ".gates.bias"] = sd[src + ".to_gates.bias"]
        out[prefix + ".out"] = sd[src + ".to_out.0.weight"].T

    def feed_forward(prefix, src):
        out[prefix + ".norm"] = sd[src + ".net.0.gamma"]
        out[prefix + ".up.weight"] = sd[src + ".net.1.weight"].T
        out[prefix + ".up.bias"] = sd[src + ".net.1.bias"]
        out[prefix + ".down.weight"] = sd[src + ".net.4.weight"].T
        out[prefix + ".down.bias"] = sd[src + ".net.4.bias"]

    for i in range(3):
        p = f"frontend.blocks.{i}.partial"
        attention(f"block{i}.attn_f", p + ".attnF")
        feed_forward(f"block{i}.ff_f", p + ".ffF")
        attention(f"block{i}.attn_t", p + ".attnT")
        feed_forward(f"block{i}.ff_t", p + ".ffT")
    out["linear.weight"] = sd["frontend.linear.weight"].T
    out["linear.bias"] = sd["frontend.linear.bias"]
    for i in range(6):
        attention(f"layer{i}.attn", f"transformer_blocks.layers.{i}.0")
        feed_forward(f"layer{i}.ff", f"transformer_blocks.layers.{i}.1")
    out["norm"] = sd["transformer_blocks.norm.gamma"]
    out["head.weight"] = sd["task_heads.beat_downbeat_lin.weight"].T
    out["head.bias"] = sd["task_heads.beat_downbeat_lin.bias"]
    # The mel filterbank of the model's input, [513 bins, 128 bands].
    from beat_this.preprocessing import LogMelSpect
    out["mel.filters"] = LogMelSpect().spect_class.mel_scale.fb
    write_mzm(os.path.join(MODELS, "beat-this-small.mzm"), out)

    # The reference: the full-precision network on a synthetic groove (the
    # stored weights are rounded to f16, so the tests allow for that).
    from beat_this.preprocessing import LogMelSpect
    audio = synthetic_groove(seconds=8.0)
    mel = LogMelSpect()(torch.tensor(audio))
    with torch.no_grad():
        pred = net(mel[None])
    return net, audio, mel, pred


def synthetic_groove(seconds, sr=22050, bpm=120.0):
    """Kick, snare and hi-hat over a C – Am – F – G progression of saw tones."""
    rng = np.random.default_rng(1)
    n = int(seconds * sr)
    y = np.zeros(n, np.float32)
    t = np.arange(int(0.2 * sr)) / sr
    kick = np.sin(2 * np.pi * (45 + 90 * np.exp(-t * 35)) * t) * np.exp(-t * 18)
    snare = rng.normal(size=len(t)) * np.exp(-t * 28) * 0.4
    hat = rng.normal(size=len(t)) * np.exp(-t * 110) * 0.15
    beat = 60.0 / bpm
    for i in range(int(seconds / beat * 2)):
        s = int((0.1 + i * beat / 2) * sr)
        if s + len(t) > n:
            break
        y[s:s + len(t)] += hat
        if i % 2 == 0:
            y[s:s + len(t)] += kick if (i // 2) % 2 == 0 else snare
    chords = [[60, 64, 67], [57, 60, 64], [53, 57, 60], [55, 59, 62]]
    bar = 4 * beat
    tt = np.arange(n) / sr
    for b in range(int(seconds / bar) + 1):
        a, z = int((0.1 + b * bar) * sr), min(int((0.1 + (b + 1) * bar) * sr), n)
        for key in chords[b % 4] + [chords[b % 4][0] - 24]:
            f = 440 * 2 ** ((key - 69) / 12)
            ph = (tt[a:z] * f) % 1.0
            y[a:z] += 0.05 * (2 * ph - 1)
    return (y / np.abs(y).max() * 0.8).astype(np.float32)


# ---------------------------------------------------------------------- BTC

def export_btc(repo):
    sys.path.insert(0, repo)
    from btc_model import BTC_model
    from utils.hparams import HParams

    config = HParams.load(os.path.join(repo, "run_config.yaml"))
    config.feature["large_voca"] = True
    config.model["num_chords"] = 170
    ck = torch.load(os.path.join(repo, "test/btc_model_large_voca.pt"), weights_only=False, map_location="cpu")
    model = BTC_model(config=config.model)
    model.load_state_dict(ck["model"])
    model.eval()
    sd = {k: v.detach() for k, v in ck["model"].items()}
    out = {"mean": torch.tensor([ck["mean"]]), "std": torch.tensor([ck["std"]])}
    p = "self_attn_layers."
    out["embed"] = sd[p + "embedding_proj.weight"].T
    for i in range(8):
        for d, block in [("fwd", "attn_block"), ("bwd", "backward_attn_block")]:
            s = f"{p}self_attn_layers.{i}.{block}."
            o = f"layer{i}.{d}."
            out[o + "norm1.gamma"] = sd[s + "layer_norm_mha.gamma"]
            out[o + "norm1.beta"] = sd[s + "layer_norm_mha.beta"]
            for w in ["query", "key", "value", "output"]:
                out[o + w] = sd[s + f"multi_head_attention.{w}_linear.weight"].T
            out[o + "norm2.gamma"] = sd[s + "layer_norm_ffn.gamma"]
            out[o + "norm2.beta"] = sd[s + "layer_norm_ffn.beta"]
            for j in range(2):
                out[o + f"conv{j}.weight"] = sd[s + f"positionwise_convolution.layers.{j}.conv.weight"]
                out[o + f"conv{j}.bias"] = sd[s + f"positionwise_convolution.layers.{j}.conv.bias"]
        out[f"layer{i}.merge.weight"] = sd[f"{p}self_attn_layers.{i}.linear.weight"].T
        out[f"layer{i}.merge.bias"] = sd[f"{p}self_attn_layers.{i}.linear.bias"]
    out["norm.gamma"] = sd[p + "layer_norm.gamma"]
    out["norm.beta"] = sd[p + "layer_norm.beta"]
    out["out.weight"] = sd["output_layer.output_projection.weight"].T
    out["out.bias"] = sd["output_layer.output_projection.bias"]
    write_mzm(os.path.join(MODELS, "btc-large.mzm"), out)

    for k, v in model.state_dict().items():
        if v.dtype == torch.float32:
            v.copy_(f16(v))
    return model, float(f16([ck["mean"]])[0]), float(f16([ck["std"]])[0])


def main():
    onnx_path, btc_repo = sys.argv[1], sys.argv[2]
    os.makedirs(MODELS, exist_ok=True)
    os.makedirs(TESTDATA, exist_ok=True)
    _, audio, mel, pred = export_beat_this(onnx_path)
    write_mzm(os.path.join(TESTDATA, "beat-this.mzm"), {
        "audio": audio, "mel": mel, "beat": pred["beat"][0], "downbeat": pred["downbeat"][0],
    }, dtype=1)

    import librosa
    model, mean, std = export_btc(btc_repo)
    y = synthetic_groove(seconds=10.0)
    cqt = librosa.cqt(y, sr=22050, n_bins=144, bins_per_octave=24, hop_length=2048)
    feat = np.log(np.abs(cqt) + 1e-6).T.astype(np.float32)
    x = torch.tensor((feat - mean) / std)[None]
    with torch.no_grad():
        h, _ = model.self_attn_layers(x)
        logits = model.output_layer.output_projection(h)
    write_mzm(os.path.join(TESTDATA, "btc.mzm"), {
        "audio": y, "tuning": np.array([librosa.estimate_tuning(y=y, sr=22050, bins_per_octave=24)], np.float32),
        "cqt": feat, "logits": logits[0],
    }, dtype=1)


if __name__ == "__main__":
    main()
