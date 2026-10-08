// SPDX-License-Identifier: MPL-2.0
// The fixed private ABI exposes no upstream configuration, callbacks or struct layout
#include <float.h>
#include <stdint.h>
#include "BTT.h"

typedef char require_ieee_f32[(sizeof(float) == 4 && FLT_RADIX == 2 && FLT_MANT_DIG == 24 && FLT_MAX_EXP == 128) ? 1 : -1];
typedef char require_ieee_f64[(sizeof(double) == 8 && DBL_MANT_DIG == 53 && DBL_MAX_EXP == 1024) ? 1 : -1];
typedef char require_i32[(sizeof(int) == 4) ? 1 : -1];

void *cocobeat_btt_48000_new(void) {
    BTT *tracker = btt_new(1024, 8, 15, 1024, 1024, 1024, 48000, 0, 0);
    if (tracker != 0) {
        btt_set_tracking_mode(tracker, BTT_ONSET_AND_TEMPO_TRACKING);
    }
    return tracker;
}

void cocobeat_btt_48000_process(void *tracker, float *samples, uint32_t frames,
                              double *bpm, int32_t *period, double *certainty) {
    btt_process((BTT *)tracker, samples, (int)frames);
    *bpm = btt_get_tempo_bpm((BTT *)tracker);
    *period = btt_get_beat_period_audio_samples((BTT *)tracker);
    *certainty = btt_get_tempo_certainty((BTT *)tracker);
}

void cocobeat_btt_48000_drop(void *tracker) {
    btt_destroy((BTT *)tracker);
}
