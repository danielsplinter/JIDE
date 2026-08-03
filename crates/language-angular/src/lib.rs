#![doc = "Tudo o que é exclusivo de Angular, e nada de Angular fora daqui."]
#![doc = ""]
#![doc = "Uma crate só, com as capacidades em módulos — a ADR-024 aplicada de novo."]
#![doc = ""]
#![doc = "# O que esta crate faz, e o que ela deliberadamente não faz"]
#![doc = ""]
#![doc = "Ela **não entende sintaxe de template**. Não há gramática de `@if` aqui,"]
#![doc = "nem parser de expressão, nem tabela de versões. Quem entende o template é"]
#![doc = "o `@angular/language-service`, que é do time do Angular e envelhece com"]
#![doc = "ele; esta crate só diz ao analisador de TypeScript onde encontrá-lo e que"]
#![doc = "um `.html` ao lado de um `.ts` é template dele."]
#![doc = ""]
#![doc = "É a diferença medida entre esta escolha e a do IntelliJ, que carrega"]
#![doc = "quatro versões do próprio parser de template — uma por revisão da sintaxe."]

mod analyzer;
mod project;

pub use analyzer::AngularAnalyzerPlugin;
pub use project::e_angular;
