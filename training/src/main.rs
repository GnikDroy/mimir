//! Checkpoints contain quantised.bin and raw.bin (f32)
//! Quantisation is lossy so fine-tune from raw.bin

use bullet::{
    game::inputs::Chess768,
    nn::optimiser::AdamW,
    trainer::{
        save::SavedFormat,
        schedule::{lr, wdl, TrainingSchedule, TrainingSteps},
        settings::LocalSettings,
    },
    value::{loader, ValueTrainerBuilder},
};

const HIDDEN_SIZE: usize = 512;
const SCALE: i32 = 400;
const QA: i16 = 255;
const QB: i16 = 64;

const NET_ID: &str = "mimir-512-v1";

const SUPERBATCHES: usize = 240;

fn main() {
    let data_files: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        !data_files.is_empty(),
        "usage: mimir-train <data.bullet.bin> [...]"
    );
    let file_refs: Vec<&str> = data_files.iter().map(String::as_str).collect();

    let mut trainer = ValueTrainerBuilder::default()
        .dual_perspective()
        .optimiser(AdamW)
        .inputs(Chess768)
        .save_format(&[
            SavedFormat::id("l0w").round().quantise::<i16>(QA),
            SavedFormat::id("l0b").round().quantise::<i16>(QA),
            SavedFormat::id("l1w").round().quantise::<i16>(QB),
            SavedFormat::id("l1b").round().quantise::<i16>(QA * QB),
        ])
        .loss_fn(|output, target| output.sigmoid().squared_error(target))
        .build(|builder, stm_inputs, ntm_inputs| {
            let l0 = builder.new_affine("l0", 768, HIDDEN_SIZE);
            let l1 = builder.new_affine("l1", 2 * HIDDEN_SIZE, 1);

            let stm_hidden = l0.forward(stm_inputs).screlu();
            let ntm_hidden = l0.forward(ntm_inputs).screlu();
            let hidden_layer = stm_hidden.concat(ntm_hidden);
            l1.forward(hidden_layer)
        });

    let schedule = TrainingSchedule {
        net_id: NET_ID.to_string(),
        eval_scale: SCALE as f32,
        steps: TrainingSteps {
            batch_size: 16_384,
            batches_per_superbatch: 6104,
            start_superbatch: 1,
            end_superbatch: SUPERBATCHES,
        },
        wdl_scheduler: wdl::ConstantWDL { value: 0.75 },
        lr_scheduler: lr::StepLR {
            start: 0.001,
            gamma: 0.1,
            step: 100,
        },
        save_rate: 40,
    };

    let settings = LocalSettings {
        threads: 6,
        test_set: None,
        output_directory: "checkpoints",
        batch_queue_size: 64,
    };

    let data_loader = loader::DirectSequentialDataLoader::new(&file_refs);

    trainer.run(&schedule, &settings, &data_loader);
}
