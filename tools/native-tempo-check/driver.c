// SPDX-License-Identifier: MPL-2.0
// Linux QA executable using the unmodified BTT tempo estimator
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <float.h>
#include <inttypes.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include "BTT.h"

enum { RATE = 48000, CHANNELS = 2, FFT = 1024, OVERLAP = 8, HOP = 128,
       FILTER_ORDER = 15, OSS = 1024, WARMUP = OSS * HOP,
       ZERO_SUPPORT = FFT + HOP + (FILTER_ORDER + 1 + OSS) * HOP };
#define MAX_FRAMES (600U * RATE)

static size_t number(const char *text, size_t limit) {
    if (!*text) goto invalid;
    for (const char *p = text; *p; ++p)
        if (*p < '0' || *p > '9') goto invalid;
    errno = 0;
    uintmax_t value = strtoumax(text, NULL, 10);
    if (!errno && value <= limit) return (size_t)value;
invalid:
    fputs("invalid integer argument\n", stderr);
    exit(2);
}

int main(int argc, char **argv) {
    if (argc != 6) {
        fputs("usage: native-tempo-check PCM CHANNEL FRAMES BLOCK NEW_OUTPUT.jsonl\n", stderr);
        return 2;
    }
    size_t channel = number(argv[2], 1), frames = number(argv[3], MAX_FRAMES);
    size_t block = number(argv[4], 1024);
    if (!frames || (block != 1 && block != 128 && block != 1024)) {
        fputs("unsupported frame count or QA block size\n", stderr);
        return 2;
    }
    FILE *input = NULL, *output = NULL;
    BTT *tracker = NULL;
    const char *error = NULL;
    size_t consumed = 0, zeros = 0, rows = 0;
    unsigned char bytes[1024 * CHANNELS * 4];
    float samples[1024];
    struct stat info;
    int fd = -1;
#define REQUIRE(condition, message) do { if (!(condition)) { error = message; goto done; } } while (0)
    REQUIRE(sizeof(float) == 4 && FLT_RADIX == 2 && FLT_MANT_DIG == 24 && FLT_MAX_EXP == 128,
            "IEEE binary32 is required");
    fd = open(argv[1], O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
    REQUIRE(fd >= 0, "cannot open regular PCM input");
    REQUIRE(!fstat(fd, &info) && S_ISREG(info.st_mode) && info.st_size >= 0
            && (uint64_t)info.st_size == (uint64_t)frames * CHANNELS * 4,
            "PCM must be a regular stereo F32LE file of the exact declared length");
    input = fdopen(fd, "rb");
    REQUIRE(input, "cannot open PCM stream");
    fd = -1;
    fd = open(argv[5], O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0600);
    REQUIRE(fd >= 0, "cannot create new output, existing paths are never replaced");
    output = fdopen(fd, "w");
    REQUIRE(output, "cannot open output stream");
    fd = -1;
    // Equal threshold / CBSS dimensions also avoid the upstream header naming mismatch
    tracker = btt_new(FFT, OVERLAP, FILTER_ORDER, OSS, 1024, 1024, RATE, 0, 0);
    REQUIRE(tracker, "native tracker allocation failed");
    btt_set_tracking_mode(tracker, BTT_ONSET_AND_TEMPO_TRACKING);
    REQUIRE(btt_get_sample_rate(tracker) == RATE, "native sample rate differs");
    fprintf(output, "{\"type\":\"header\",\"algorithm\":\"btt-48k-tempo-only\","
        "\"sample_rate\":%d,\"channels\":%d,\"channel\":%zu,\"sample_frames\":%zu,"
        "\"block_frames\":%zu,\"fft_frames\":%d,\"hop_frames\":%d,\"oss_frames\":%d,"
        "\"filter_order\":%d,\"warmup_frames\":%d,\"zero_support_frames\":%d,"
        "\"latency_adjustments\":[0,0],\"callbacks\":false,\"confidence\":null,"
        "\"coordinate\":\"consumed_pcm_frames_not_event_position\","
        "\"zero_test\":\"exact_selected_channel_pcm_not_perceptual_silence\"}\n",
        RATE, CHANNELS, channel, frames, block, FFT, HOP, OSS, FILTER_ORDER, WARMUP, ZERO_SUPPORT);

    while (consumed < frames) {
        size_t count = frames - consumed < block ? frames - consumed : block;
        REQUIRE(fread(bytes, 1, count * CHANNELS * 4, input) == count * CHANNELS * 4,
                "truncated PCM read");
        for (size_t i = 0; i < count * CHANNELS; ++i) {
            const unsigned char *b = bytes + 4 * i;
            uint32_t bits = (uint32_t)b[0] | ((uint32_t)b[1] << 8)
                | ((uint32_t)b[2] << 16) | ((uint32_t)b[3] << 24);
            float value;
            memcpy(&value, &bits, sizeof(value));
            REQUIRE(isfinite(value) && fabsf(value) <= 4, "non-finite or out-of-range PCM");
            if (i % CHANNELS == channel) {
                samples[i / CHANNELS] = value;
                zeros = value == 0 ? zeros + 1 : 0;
            }
        }
        btt_process(tracker, samples, (int)count);
        consumed += count;
        if (consumed % HOP == 0 || consumed == frames) {
            double bpm = btt_get_tempo_bpm(tracker);
            double certainty = btt_get_tempo_certainty(tracker);
            int period = btt_get_beat_period_audio_samples(tracker);
            REQUIRE(isfinite(bpm) && isfinite(certainty) && certainty >= 0,
                    "native estimator returned a non-finite or negative value");
            REQUIRE((bpm == 0 && period == 0) || (bpm >= btt_get_min_tempo(tracker)
                    && bpm <= btt_get_max_tempo(tracker) && period > 0 && period % HOP == 0
                    && fabs(bpm - 60.0 * RATE / period) <= 1e-6 * bpm),
                    "native tempo and period disagree or exceed configured bounds");
            REQUIRE(consumed >= WARMUP || (bpm == 0 && period == 0),
                    "native tempo unexpectedly precedes its OSS warmup");
            const char *stale = !zeros ? "not_in_exact_zero_run" : !period ? "no_retained_tempo"
                : zeros >= ZERO_SUPPORT ? "stale_after_full_zero_support" : "insufficient_zero_history";
            fprintf(output, "{\"type\":\"tempo\",\"consumed_frames\":%zu,\"bpm\":%.17g,"
                "\"period_frames\":%d,\"native_certainty\":%.17g,\"confidence\":null,"
                "\"warmup_complete\":%s,\"trailing_zero_frames\":%zu,\"stale_status\":\"%s\"}\n",
                consumed, bpm, period, certainty, consumed >= WARMUP ? "true" : "false", zeros, stale);
            ++rows;
            REQUIRE(!ferror(output), "output write failed, partial artifact retained");
        }
    }
    REQUIRE(fgetc(input) == EOF && !ferror(input), "PCM changed length or read failed");
    {
        int close_failed = fclose(input);
        input = NULL;
        REQUIRE(!close_failed, "PCM close failed");
    }
    fprintf(output, "{\"type\":\"complete\",\"sample_frames\":%zu,\"records\":%zu}\n", frames, rows);
    REQUIRE(!ferror(output), "output write failed, partial artifact retained");
done:
    btt_destroy(tracker);
    if (fd >= 0) close(fd);
    if (input && fclose(input) && !error) error = "PCM close failed";
    if (output && fclose(output) && !error) error = "output close failed";
    if (error) fprintf(stderr, "%s\n", error);
    return error ? 2 : 0;
}
