use anyhow::Result;
use canvas_core::{
    Command, CropRect, Document, Group, ImageContent, LayerContent, ShapeContent, TextContent,
    Transform, VideoContent,
};
use canvas_render::{image_data_from_rgba, ImageMap};

pub(super) fn build() -> Result<(Document, ImageMap)> {
    let mut doc = Document::new(1920.0, 1080.0);
    doc.page_mut()?.background = Some([20, 40, 60, 255]);
    let rgba = [90, 180, 140, 255].repeat(64 * 64);
    let id = doc.add_layer(
        "Image",
        Transform::new(100.0, 100.0, 1720.0, 880.0),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 64,
            natural_height: 64,
            crop: None,
        }),
    )?;
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(rgba, 64, 64));
    let title = doc.add_layer(
        "Title",
        Transform::new(650.0, 200.0, 620.0, 140.0),
        LayerContent::Text(TextContent {
            text: "Framing 9:16".into(),
            size: 90.0,
            color: [255, 255, 255, 255],
            ..TextContent::default()
        }),
    )?;
    let shape = doc.add_layer(
        "Shape",
        Transform::new(860.0, 500.0, 200.0, 200.0),
        LayerContent::Shape(ShapeContent {
            fill: [240, 70, 90, 255],
            ..ShapeContent::default()
        }),
    )?;
    let group = doc.allocate_layer_id();
    Group::new(vec![title, shape], group, "Grouped title and shape").apply(&mut doc)?;
    extras(&mut doc, &mut images)?;
    Ok((doc, images))
}

fn extras(doc: &mut Document, images: &mut ImageMap) -> Result<()> {
    let crop = doc.add_layer(
        "Cropped image",
        Transform::new(200.0, 750.0, 200.0, 150.0),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 4,
            natural_height: 2,
            crop: Some(CropRect {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            }),
        }),
    )?;
    let mut rgba = Vec::new();
    for _ in 0..2 {
        for x in 0..4 {
            rgba.extend_from_slice(if x < 2 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
    }
    images.insert(crop, image_data_from_rgba(rgba, 4, 2));
    let fx = doc.add_layer(
        "Grayscale blur",
        Transform::new(500.0, 750.0, 200.0, 150.0),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 64,
            natural_height: 64,
            crop: None,
        }),
    )?;
    images.insert(
        fx,
        image_data_from_rgba([180, 60, 30, 255].repeat(64 * 64), 64, 64),
    );
    doc.layer_mut(fx)?.effects.grayscale = 1.0;
    doc.layer_mut(fx)?.effects.blur_radius = 4.0;
    video_and_visibility(doc, images)
}

fn video_and_visibility(doc: &mut Document, images: &mut ImageMap) -> Result<()> {
    let video = doc.add_layer(
        "Paused video poster",
        Transform::new(1100.0, 750.0, 200.0, 150.0),
        LayerContent::Video(VideoContent {
            source_path: None,
            natural_width: 64,
            natural_height: 64,
            crop: None,
            duration_secs: Some(10.0),
            poster_time: 2.0,
            trim_start: 0.0,
            trim_end: None,
        }),
    )?;
    images.insert(
        video,
        image_data_from_rgba([220, 190, 30, 255].repeat(64 * 64), 64, 64),
    );
    let transparent = doc.add_layer(
        "Transparent shape",
        Transform::new(1400.0, 750.0, 200.0, 150.0),
        LayerContent::Shape(ShapeContent {
            fill: [255, 255, 255, 255],
            ..Default::default()
        }),
    )?;
    doc.layer_mut(transparent)?.opacity = 0.5;
    let hidden = doc.add_layer(
        "Hidden shape",
        Transform::new(200.0, 400.0, 100.0, 100.0),
        LayerContent::Shape(ShapeContent {
            fill: [255, 0, 255, 255],
            ..Default::default()
        }),
    )?;
    doc.layer_mut(hidden)?.visible = false;
    Ok(())
}

pub(super) fn check(rgba: &[u8], width: u32) -> Result<()> {
    let px =
        |x: usize, y: usize| &rgba[(y * width as usize + x) * 4..(y * width as usize + x) * 4 + 4];
    anyhow::ensure!(px(5, 5) == [20, 40, 60, 255], "background missing");
    anyhow::ensure!(px(110, 110) == [90, 180, 140, 255], "image missing");
    anyhow::ensure!(px(900, 600) == [240, 70, 90, 255], "grouped shape missing");
    anyhow::ensure!(px(250, 450) == [90, 180, 140, 255], "hidden layer rendered");
    anyhow::ensure!(px(350, 800) == [0, 0, 255, 255], "image crop missing");
    let gray = px(600, 825);
    anyhow::ensure!(
        gray[0].abs_diff(gray[1]) <= 1 && gray[1].abs_diff(gray[2]) <= 1,
        "grayscale effect missing"
    );
    anyhow::ensure!(
        px(1200, 825) == [220, 190, 30, 255],
        "paused video poster missing"
    );
    let transparent = px(1500, 825);
    anyhow::ensure!(
        transparent[0] > 90 && transparent[0] < 255 && transparent[3] == 255,
        "opacity missing"
    );
    let white = (200..340)
        .flat_map(|y| (650..1270).map(move |x| (x, y)))
        .filter(|&(x, y)| px(x, y)[..3].iter().all(|c| *c > 245))
        .count();
    anyhow::ensure!(white > 100, "grouped text missing");
    Ok(())
}
