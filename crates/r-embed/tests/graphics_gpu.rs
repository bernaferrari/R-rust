#![cfg(feature = "vello-gpu")]
use r_embed::{GpuRenderer, RSession};

#[test]
#[ignore = "requires a compute-capable GPU"]
fn r_scene_survives_session_close_and_renders_on_gpu() {
    let mut session = RSession::new().unwrap();
    let scene = session.record_scene("x<-1:20;y<-sin(x/3);fit<-loess(y~x);plot(x,y,main=expression(frac(alpha[1]^2,sqrt(beta))));lines(x,predict(fit),col='red')",640,480).unwrap();
    let grid = session.record_scene("library(grid); grid.newpage(); pushViewport(viewport(width=.6,height=.6)); grid.draw(grobTree(rectGrob(gp=gpar(fill='blue',col=NA)),textGrob(expression(sum(x[i],i==1,n)),gp=gpar(col='white',fontsize=24)))); popViewport()",320,240).unwrap();
    drop(session);
    pollster::block_on(async {
        let mut gpu = GpuRenderer::new().await.unwrap();
        eprintln!("R integration GPU: {:?}", gpu.adapter_info());
        let png = gpu.render_png(&scene).await.unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(&png))
            .read_info()
            .unwrap();
        let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut rgba).unwrap();
        assert!(
            rgba.chunks_exact(4)
                .filter(|p| p[0] < 100 && p[1] < 100 && p[2] < 100)
                .count()
                > 300
        );
        assert!(
            rgba.chunks_exact(4)
                .filter(|p| p[0] > 180 && p[1] < 100 && p[2] < 100)
                .count()
                > 50
        );
        let grid_pixels = gpu.render_rgba(&grid).await.unwrap();
        assert!(
            grid_pixels
                .chunks_exact(4)
                .filter(|p| p[2] > 200 && p[0] < 30 && p[1] < 30)
                .count()
                > 10_000
        );
        if let Ok(path) = std::env::var("RPORT_GPU_ARTIFACT") {
            std::fs::write(path, png).unwrap();
        }
    });
}
