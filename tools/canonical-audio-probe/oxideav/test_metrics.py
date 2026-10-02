"""The nonfinite failure must remain serializable without changing finite metrics"""
import json
import math
from pathlib import Path
import struct
import tempfile
from probe import metrics

with tempfile.TemporaryDirectory() as temporary:
    raw = Path(temporary) / 'samples.f32le'
    raw.write_bytes(struct.pack('<ffff', 0.25, -0.5, 0.0, 0.5))
    finite = metrics(raw, [[0.25, 0.0], [-0.5, 0.5]])
    assert finite['channels'][0]['peak'] == 0.25
    assert finite['channels'][0]['rms'] == math.sqrt(0.25**2 / 2)
    assert finite['channels'][0]['rmse_aligned'] == 0
    for bad in [float('nan'), float('inf'), -float('inf')]:
        raw.write_bytes(struct.pack('<ffff', bad, -0.5, 0.0, 0.5))
        result = metrics(raw, [[0.25, 0.0], [-0.5, 0.5]])
        assert result['channels'][0]['nonfinite'] == 1
        assert result['channels'][0]['rmse_aligned'] is None
        assert result['channels'][0]['first_8'][0] is None
        json.dumps(result, allow_nan=False)
print('PASS: finite values unchanged; NaN and infinities record a serializable failure')
