use remu_audio::events;
use remu_audio::player;

use anyhow::Result;
use player::Player;
use std::thread;
use std::time::Duration;

use events::PlayerEvent;
use player::PlaybackControl;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化播放器
    let mut player = Player::new()?;

    let control = player.control();

    player.set_callback(move |event| {
        // 设置事件监听器 - 使用 UI 显示
        match event {
            PlayerEvent::Play => {
                println!("[@Play] 播放开始");
            }
            PlayerEvent::Pause => {
                println!("[@Pause] 播放已暂停");
            }
            PlayerEvent::Playing => {
                println!("[@Playing] 正在播放");
            }
            PlayerEvent::Emptied => {
                println!("[@Emptied] 媒体内容已清空");
            }
            PlayerEvent::Ended => {
                println!("[@Ended] 播放完成");
            }
            PlayerEvent::Waiting => {
                println!("[@Waiting] 缓冲中...");
            }
            PlayerEvent::DurationChange => {
                let cl = control.read().unwrap();
                let duration = cl.duration();
                drop(cl);
                if let Some(d) = duration {
                    println!("[@DurationChange] 时长: {:.1} 秒", d.as_secs_f32());
                } else {
                    println!("[@DurationChange] 时长: 未知");
                }
            }
            PlayerEvent::VolumeChange => {
                let cl = control.read().unwrap();
                let volume = cl.volume();
                drop(cl);
                println!("[@VolumeChange] 音量: {:.0}%", volume * 100.0);
            }
            PlayerEvent::Seeking => {
                println!("[@Seeking] 正在跳转...");
            }
            PlayerEvent::Seeked => {
                println!("[@Seeked] 跳转完成");
            }
            PlayerEvent::LoadStart => {
                println!("[@LoadStart] 开始加载文件...");
            }
            PlayerEvent::LoadedData => {
                println!("[@LoadedData] 已加载首帧数据");
            }
            PlayerEvent::LoadedMetadata => {
                println!("[@LoadedMetadata] 元数据加载完成，准备播放");
            }
            PlayerEvent::Error { message } => {
                println!("[@Error] 错误: {}", message);
            }
        }
    });

    player.set_loader_callback(move |event| match event {
        remu_audio::loader::LoaderEvent::Completed => {
            println!("[@Loader:Completed] 内容加载完成")
        }
        remu_audio::loader::LoaderEvent::Aborted => {
            println!("[@Loader:Aborted] 内容加载中止")
        }
    });

    // 加载音频文件
    let file_path = "./audio-example/ARForest - Art for Rest.mp3";
    println!("当前文件: {}", file_path);
    println!("状态: 正在加载文件...");

    match player.load_file(file_path).await {
        Ok(_) => {
            println!("文件加载成功");
        }
        Err(e) => {
            println!("文件加载失败: {}", e);
            return Ok(());
        }
    }

    // 设置音量
    println!("\n设置音量\n");

    player.set_volume(0.5);
    thread::sleep(Duration::from_millis(200));

    // 开始播放
    println!("\n开始播放\n");

    player.play();
    thread::sleep(Duration::from_millis(1000));

    println!("测试暂停");
    player.pause();
    thread::sleep(Duration::from_secs(2));
    player.play();

    thread::sleep(Duration::from_secs(5));

    println!("测试跳转");
    let _ = player.seek(Duration::from_secs(20));

    thread::sleep(Duration::from_secs(5));

    println!("测试音量调整");
    player.set_volume(0.8);

    thread::sleep(Duration::from_secs(5));

    let file_path = "./audio-example/1mb.mp3";
    println!("当前文件: {}", file_path);
    println!("状态: 正在加载文件...");
    match player.load_file(file_path).await {
        Ok(_) => {
            println!("文件加载成功");
        }
        Err(e) => {
            println!("文件加载失败: {}", e);
            return Ok(());
        }
    }
    thread::sleep(Duration::from_secs(10));

    // 加载网络音频文件
    let file_url = "https://download.samplelib.com/mp3/sample-15s.mp3";

    println!("加载文件开始: {}", file_url);
    player.load_url(file_url).await?;
    thread::sleep(Duration::from_secs(20));

    println!("测试完成！");

    Ok(())
}
