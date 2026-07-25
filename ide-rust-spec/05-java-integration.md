# 05 — Integração Java Inicial

## Objetivo

Fornecer suporte Java sem usar uma JVM embutida na IDE.

A IDE deverá:

- analisar Java nativamente em Rust;
- ler `.class` e `.jar`;
- detectar JDKs instalados;
- permitir selecionar o JDK usado pelo projeto;
- usar o SDK do WebSphere quando configurado;
- iniciar `javac`, Maven, Gradle e ferramentas Java apenas sob demanda.

## Componentes

```text
JavaLanguageProvider
├── JavaSyntaxEngine
├── JavaSemanticEngine
├── JavaClassFileReader
├── JavaSymbolIndexer
├── JavaCompletionEngine
├── JavaDiagnosticsEngine
└── JavaRefactoringEngine

JavaToolchainProvider
├── OracleJdkDetector
├── OpenJdkDetector
└── WebSphereSdkDetector

JavaCompilerAdapter
└── JavacProcessAdapter

JavaBuildAdapters
├── MavenAdapter
└── GradleAdapter

JavaRuntimeAdapters
├── JavaProcessAdapter
└── WebSphereRuntimeAdapter
```

## Configuração

```toml
[languages.java]
enabled = true
provider = "native-java"

[toolchains.java]
home = "C:/IBM/WebSphere/AppServer/java/8.0"
source = 8
target = 8

[servers.websphere]
home = "C:/IBM/WebSphere/AppServer"
profile = "AppSrv01"
server = "server1"
```

## Detecção do WebSphere

O adapter deve procurar:

```text
WAS_HOME/bin/managesdk.bat
WAS_HOME/java
WAS_HOME/java/8.0
WAS_HOME/profiles
WAS_HOME/plugins
WAS_HOME/dev
```

Validação mínima:

```text
JAVA_HOME/bin/java
JAVA_HOME/bin/javac
JAVA_HOME/bin/jar
```

## Compatibilidade Java 8

O suporte inicial deverá incluir:

- classes;
- interfaces;
- enums;
- annotations;
- generics;
- lambdas;
- method references;
- streams;
- imports;
- inner classes;
- anonymous classes;
- try-with-resources;
- default methods;
- static interface methods.

## Bibliotecas padrão

Para Java 8:

```text
JAVA_HOME/jre/lib/rt.jar
JAVA_HOME/jre/lib/*.jar
```

O analisador deve indexar APIs públicas e metadados necessários.

## Bibliotecas WebSphere

O classpath do projeto poderá incluir bibliotecas fornecidas pelo servidor:

```text
WAS_HOME/plugins
WAS_HOME/dev
WAS_HOME/lib
```

Não indexar indiscriminadamente todos os JARs. O adapter de projeto deve determinar quais bibliotecas realmente pertencem ao classpath.

## Maven

Estratégia:

1. interpretar o `pom.xml` básico nativamente;
2. executar Maven externo para obter o modelo efetivo quando necessário;
3. configurar `JAVA_HOME` com o JDK selecionado;
4. importar dependências e módulos;
5. acompanhar mudanças no POM.

## Gradle

Gradle deve ser tratado como ferramenta externa.

A IDE não deve tentar interpretar toda lógica Groovy ou Kotlin.

## WebSphere

Operações iniciais:

- detectar instalação;
- listar perfis;
- listar servidores;
- iniciar;
- parar;
- reiniciar;
- acompanhar logs;
- publicar artefato;
- remover artefato;
- conectar depurador remoto;
- executar scripts `wsadmin`.

## Segurança

- nunca compartilhar memória com a JVM do servidor;
- nunca carregar bibliotecas do WebSphere no processo principal;
- executar comandos por adapter;
- validar caminhos;
- escapar argumentos;
- registrar comandos executados sem expor segredos.
