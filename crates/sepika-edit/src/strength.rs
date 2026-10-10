use super::*;
use sepika_core::model::{Material, SourceStory, StbStrengthInput};

/// STB 強度の元指定を置換する。参照不整合はモデルを変更しない。
pub struct SetStbStrengths {
    pub input: StbStrengthInput,
}
impl EditCommand for SetStbStrengths {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let mut candidate = model.clone();
        candidate.stb_strengths = self.input.clone();
        candidate.stb_strengths.materials = model.stb_strengths.materials.clone();
        candidate.prepare_stb_strength_materials();
        if candidate.validate().is_err() {
            return Box::new(Noop);
        }
        let inverse = RestoreStbStrengths {
            input: model.stb_strengths.clone(),
            materials: model.materials.clone(),
        };
        model.stb_strengths = candidate.stb_strengths;
        model.materials = candidate.materials;
        Box::new(inverse)
    }
    fn label(&self) -> &str {
        "STB強度指定変更"
    }
}
struct RestoreStbStrengths {
    input: StbStrengthInput,
    materials: Vec<Material>,
}
impl EditCommand for RestoreStbStrengths {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let inverse = Self {
            input: model.stb_strengths.clone(),
            materials: model.materials.clone(),
        };
        model.stb_strengths = self.input.clone();
        model.materials = self.materials.clone();
        Box::new(inverse)
    }
    fn label(&self) -> &str {
        "STB強度指定変更の取り消し"
    }
}

pub(crate) struct RestoreStrengthInput {
    pub input: StbStrengthInput,
    pub inverse: Box<dyn EditCommand>,
    pub stories: Vec<SourceStory>,
}
impl EditCommand for RestoreStrengthInput {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let previous = model.stb_strengths.clone();
        let stories = model.source_stories.clone();
        let inverse = self.inverse.apply(model);
        model.stb_strengths = self.input.clone();
        model.source_stories = self.stories.clone();
        Box::new(Self {
            input: previous,
            inverse,
            stories,
        })
    }
    fn label(&self) -> &str {
        self.inverse.label()
    }
}

/// 原階IDを指定し、元Fc指定（省略を含む）を変更する。
pub struct SetSourceStoryConcreteStrength {
    pub source_story: u32,
    pub strength: Option<String>,
}
struct RestoreSourceStrength {
    stories: Vec<SourceStory>,
    input: StbStrengthInput,
    materials: Vec<Material>,
}
impl EditCommand for SetSourceStoryConcreteStrength {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let Some(index) = model
            .source_stories
            .iter()
            .position(|s| s.id == self.source_story)
        else {
            return Box::new(Noop);
        };
        let inverse = RestoreSourceStrength {
            stories: model.source_stories.clone(),
            input: model.stb_strengths.clone(),
            materials: model.materials.clone(),
        };
        model.source_stories[index].strength_concrete = self.strength.clone();
        model.prepare_stb_strength_materials();
        Box::new(inverse)
    }
    fn label(&self) -> &str {
        "原階Fc指定変更"
    }
}
impl EditCommand for RestoreSourceStrength {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let inverse = Self {
            stories: model.source_stories.clone(),
            input: model.stb_strengths.clone(),
            materials: model.materials.clone(),
        };
        model.source_stories = self.stories.clone();
        model.stb_strengths = self.input.clone();
        model.materials = self.materials.clone();
        Box::new(inverse)
    }
    fn label(&self) -> &str {
        "原階Fc指定変更の取り消し"
    }
}
