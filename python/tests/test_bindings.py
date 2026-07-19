import logging

import chuda


def image():
    return chuda.Image.from_rgba(8, 8, bytes([20, 40, 60, 255]) * 64)


def test_structured_frame_and_ansi():
    frame = chuda.Renderer("cpu").render(image(), 1)
    assert (frame.columns, frame.rows, frame.backend) == (1, 1, "cpu")
    assert len(frame.glyphs) == 1
    assert len(frame.foreground) == 3
    assert len(frame.background) == 3
    assert len(frame.background_transparent) == 1
    assert frame.to_ansi().endswith(b"\x1b[0m\n")


def test_batch_matches_individual_frames():
    source = image()
    renderer = chuda.Renderer("cpu")
    batch = renderer.render_many([(source, 1), (source, 2)])
    assert [frame.to_ansi() for frame in batch] == [
        renderer.render(source, 1).to_ansi(),
        renderer.render(source, 2).to_ansi(),
    ]


def test_rgba_accepts_the_buffer_protocol():
    data = memoryview(bytearray([20, 40, 60, 255]) * 64)
    assert chuda.Image.from_rgba(8, 8, data).width == 8


def test_auto_logs_one_fallback_warning(caplog):
    caplog.set_level(logging.WARNING, logger="chuda")
    renderer = chuda.Renderer("auto")
    renderer.render(image(), 1)
    renderer.render(image(), 1)
    assert len([record for record in caplog.records if "falling back to CPU" in record.message]) == 1
