#ifdef __cplusplus
extern "C" {
#endif

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct PicooSenderStats {
  uint64_t access_units;
  uint64_t packets;
  uint64_t bytes;
  uint64_t sent_datagrams;
} PicooSenderStats;

typedef struct PicooTrustedDevice {
  uint8_t device_id[64];
  uint8_t device_name[64];
  uint8_t certificate_fingerprint[128];
  uint64_t paired_at_ms;
  uint64_t last_connected_at_ms;
} PicooTrustedDevice;

typedef struct PicooDiscoveredReceiver {
  uint8_t receiver_id[64];
  uint8_t display_name[64];
  uint8_t host[64];
  uint16_t quic_port;
} PicooDiscoveredReceiver;

/**
 * Returns protocol version string for FFI smoke tests.
 */
const char *picoo_protocol_version(void);

/**
 * Create a sender session (packetization + QUIC transport).
 */
void *picoo_sender_create(void);

/**
 * Destroy a sender session created by [`picoo_sender_create`].
 */
void picoo_sender_destroy(void *handle);

/**
 * Connect QUIC session to host:port (PCP/1 ALPN `picoocam/1`).
 */
int32_t picoo_sender_connect(void *handle, const char *host, uint16_t port);

/**
 * User-initiated disconnect (no auto-reconnect until the next connect). PUC-005.
 */
int32_t picoo_sender_disconnect(void *handle);

/**
 * Drive QUIC I/O (call periodically from platform thread).
 */
int32_t picoo_sender_pump(void *handle);

/**
 * Ingest one H.264 access unit. Returns 0 on success, negative on error.
 */
int32_t picoo_sender_ingest_access_unit(void *handle,
                                        const uint8_t *data,
                                        uintptr_t len,
                                        uint8_t is_keyframe,
                                        uint64_t pts_us,
                                        uint32_t stream_epoch,
                                        uint32_t *out_packets);

/**
 * Flush pending VideoPackets over QUIC datagrams.
 */
int32_t picoo_sender_flush(void *handle, uint32_t *out_sent);

/**
 * Read cumulative sender stats.
 */
int32_t picoo_sender_stats(void *handle, struct PicooSenderStats *out);

/**
 * Number of VideoPackets waiting for transport.
 */
uint64_t picoo_sender_pending_packets(void *handle);

/**
 * Current sender session status (see `PicooSenderStatus` values).
 */
int32_t picoo_sender_status(void *handle);

/**
 * Mark Permission Required (REQ-PICOO-SESSION-001). Returns 0 on success.
 */
int32_t picoo_sender_mark_permission_required(void *handle);

/**
 * Clear Permission Required after the host grants access. Returns 0 on success.
 */
int32_t picoo_sender_clear_permission_required(void *handle);

/**
 * Send ClientHello after QUIC connect (PUC-001).
 *
 * `qr_nonce` may be null/empty for mDNS; required to match receiver active QR for QR path.
 */
int32_t picoo_sender_send_client_hello(void *handle,
                                       const char *sender_id,
                                       const char *device_name,
                                       const uint8_t *public_key,
                                       uintptr_t public_key_len,
                                       const char *qr_nonce);

/**
 * Send PairingConfirm after desktop confirms six-digit code.
 */
int32_t picoo_sender_send_pairing_confirm(void *handle, const char *receiver_id);

/**
 * Copy pairing short code into `out` buffer. Returns length, 0 if none, negative on error.
 */
int32_t picoo_sender_pairing_short_code(void *handle, char *out, uintptr_t out_len);

/**
 * Configure stream parameters before/at streaming (PUC-005 / REQ-PICOO-PROTOCOL-005).
 *
 * `sps`/`pps` may be null/0 when unknown. Prefer NAL payloads without start codes;
 * Annex-B blobs are also accepted when both parameter sets are present in one buffer
 * passed via `sps` (with `pps` empty) — see `picoo_h264_extract_sps_pps`.
 */
int32_t picoo_sender_set_stream_config(void *handle,
                                       uint32_t width,
                                       uint32_t height,
                                       uint32_t fps,
                                       uint32_t bitrate_bps,
                                       uint32_t stream_epoch,
                                       uint8_t mirrored,
                                       uint32_t rotation,
                                       const uint8_t *sps,
                                       uintptr_t sps_len,
                                       const uint8_t *pps,
                                       uintptr_t pps_len);

/**
 * Extract SPS/PPS from Annex-B or AVCC bytes into caller buffers (REQ-PICOO-PROTOCOL-005).
 *
 * Returns 0 on success, negative on error. On success writes lengths into `*_len` in/out.
 */
int32_t picoo_h264_extract_sps_pps(const uint8_t *data,
                                   uintptr_t data_len,
                                   uint8_t *sps_out,
                                   uintptr_t *sps_len,
                                   uint8_t *pps_out,
                                   uintptr_t *pps_len);

/**
 * Current adaptive bitrate in bps.
 */
uint32_t picoo_sender_current_bitrate_bps(void *handle);

/**
 * Latest ReceiverStats feedback for live UI (PUC-005 / REQ-PICOO-PROTOCOL-006).
 *
 * Writes `[rtt_ms, packet_loss, jitter_ms, frame_age_ms, receive_bitrate, jitter_depth_ms]`
 * into `out` (length 6). Returns 0 when stats are available, 1 when none yet, -1 on error.
 */
int32_t picoo_sender_last_receiver_stats(void *handle, double *out, uintptr_t out_len);

/**
 * Returns 1 if receiver requested an IDR (consumes the flag). REQ-PICOO-SESSION-003.
 */
int32_t picoo_sender_take_keyframe_request(void *handle);

/**
 * Returns 1 if ABR asks the host to drop resolution (consumes the flag). REQ-PICOO-MEDIA-010.
 */
int32_t picoo_sender_take_resolution_downshift(void *handle);

/**
 * Returns 1 if ABR asks the host to restore preferred resolution (consumes the flag).
 */
int32_t picoo_sender_take_resolution_upshift(void *handle);

/**
 * Set preferred capture height for ABR upshift decisions (720 or 1080).
 */
int32_t picoo_sender_set_preferred_height(void *handle, uint32_t height);

/**
 * Max height advertised by receiver Capabilities (0 if unknown). REQ-PICOO-MEDIA-002.
 */
uint32_t picoo_sender_receiver_max_height(void *handle);

/**
 * Attach trusted device store path to sender (load + auto-save on pairing).
 */
int32_t picoo_sender_attach_trusted_store(void *handle, const char *path);

/**
 * Connected receiver id from ServerHello / pairing state.
 */
int32_t picoo_sender_connected_receiver_id(void *handle, char *out, uintptr_t out_len);

/**
 * Load trusted device store from JSON path.
 */
void *picoo_trusted_store_load(const char *path);

void picoo_trusted_store_destroy(void *handle);

uint32_t picoo_trusted_store_count(void *handle);

int32_t picoo_trusted_store_get(void *handle, uint32_t index, struct PicooTrustedDevice *out);

/**
 * Remove device by id. Returns 1 if removed, 0 if not found, negative on error.
 */
int32_t picoo_trusted_store_remove(void *handle, const char *device_id);

/**
 * Clear every trusted device. Returns the number removed (≥0), or negative on error.
 */
int32_t picoo_trusted_store_clear(void *handle);

int32_t picoo_trusted_store_save(void *handle);

/**
 * Load or create durable sender identity at `path` (REQ-PICOO-PAIRING-001).
 */
void *picoo_identity_load_or_create(const char *path, const char *default_name);

void picoo_identity_destroy(void *handle);

int32_t picoo_identity_device_id(void *handle, char *out, uintptr_t out_len);

int32_t picoo_identity_device_name(void *handle, char *out, uintptr_t out_len);

/**
 * Copy public key bytes into `out`. Returns length, or negative on error.
 */
int32_t picoo_identity_public_key(void *handle, uint8_t *out, uintptr_t out_len);

/**
 * Persist identity after renaming display name.
 */
int32_t picoo_identity_set_device_name(void *handle, const char *name, const char *path);

/**
 * Create mDNS browser for receiver discovery (PUC-002).
 */
void *picoo_discovery_browser_create(void);

void picoo_discovery_browser_destroy(void *handle);

/**
 * Poll mDNS events; refreshes cached receiver list.
 */
int32_t picoo_discovery_browser_poll(void *handle, uint32_t timeout_ms);

uint32_t picoo_discovery_browser_count(void *handle);

int32_t picoo_discovery_browser_get(void *handle,
                                    uint32_t index,
                                    struct PicooDiscoveredReceiver *out);

/**
 * Export redacted diagnostics JSON to file — REQ-PICOO-PRIVACY-003.
 */
int32_t picoo_export_diagnostics_to_path(const char *trusted_store_path,
                                         const char *platform,
                                         const char *app_version,
                                         const char *out_path);

/**
 * Copy redacted diagnostics JSON into `out` buffer. Returns byte length, negative on error.
 */
int32_t picoo_export_diagnostics_json(const char *trusted_store_path,
                                      const char *platform,
                                      const char *app_version,
                                      char *out,
                                      uintptr_t out_len);

/**
 * QR JSON connect payload parse helper — returns host/port/receiver_id/nonce or negative on error.
 * Returns -4 if payload is expired (REQ-PICOO-DISCOVERY-004).
 */
int32_t picoo_qr_connect_parse(const char *json,
                               char *out_host,
                               uintptr_t out_host_len,
                               uint16_t *out_port,
                               char *out_receiver_id,
                               uintptr_t out_receiver_id_len,
                               uint64_t *out_expires_at_ms,
                               char *out_nonce,
                               uintptr_t out_nonce_len);

#ifdef __cplusplus
}
#endif
