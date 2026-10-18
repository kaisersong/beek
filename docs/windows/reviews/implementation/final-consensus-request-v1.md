# 最终实现共识请求 v1

用户要求Codex+ZCode对抗后收敛。四个实际审查会话分别完成源代码子系统评审，并在后续实测修订后关闭异议。请基于既有评审结论与本附件，作最终“有限源码共识PASS / REVISE”判断。明确引用最终manifest SHA和source_fingerprint；不声称你独立重算hash，也不声称逐行独立审阅未附整个仓库。正式G0-G5尚未全通过，本次不请求发布批准。

下面列出完整最终文件散列清单和各子系统最终裁定。源码变化的覆盖链：selection R3；storage R2，后续纯空白key/30秒GPU冷启动边界已在UI-lifecycle R4全调用点复核；network-packaging R3；UI R3→R4→R5，R5最后Minor已CLOSED。最后新增仅测试入口参数帮助、产品runner、受限桌面驱动的Explorer守卫，不改变产品运行契约。Explorer实机重启因检测到可见非shell窗口被守卫拒绝，正确保持待测。

请寻找证据矛盾和遗漏，若无新的可触发源码问题，签有限共识PASS，列出尚缺环境验收。不要重复已关闭的依赖缺失假设。

## final-source-manifest-v1.json
{
  "schema": 1,
  "baseline_commit": "09cfe6d3df02ddbd5bc274e1590d6f8ec757bad6",
  "source_fingerprint": "6110fb9473824296a8173648e9fc7eb743a5b70900beb6a2548483a3c4b30f68",
  "shipping_build_id": "69e09ecbc3a9ca689a5b613564e67c7e8c6cd25205936a6b318ceab864d71d49",
  "plan_sha256": "204073460c96a1c08c786759a4ea571a795e00d16fa9d5ac3e45916e02151951",
  "files": [
    {
      "path": ".cargo/config.toml",
      "sha256": "53329a3ce3b81a3bcb1c083a46a8d47a09a459b457c8f2f5441fd13d9445d8b3"
    },
    {
      "path": ".gitattributes",
      "sha256": "fffb37e18880bd7672f6baf5f5e3ac38f0d4375e7967dec8abbc554d0750c6c8"
    },
    {
      "path": ".github/workflows/release.yml",
      "sha256": "9ff0797d9d51ab42ae7bccb7862d1c336ca2ac0398152b33dd25c2dd2d19537e"
    },
    {
      "path": ".github/workflows/windows-g0.yml",
      "sha256": "b9e7de4ad8c1e9eebdeb6f5b4f1e6ee0e4e905ca9eab25d75df8ac748c2d310c"
    },
    {
      "path": ".github/workflows/windows-product.yml",
      "sha256": "79dfc503b7119ad2d23224473cdafa7b57c4642fb0a45bd95a763f29be0e4b7a"
    },
    {
      "path": "Cargo.lock",
      "sha256": "149d44b4ed6b28611ea6f8ada5adea68dd4d38321ec74b5bdb9814376bb1e6b0"
    },
    {
      "path": "Cargo.toml",
      "sha256": "4d0e96caee234ba5fe7e5b3d29f6f40758bc0ebe03f779a95caa29025d79af76"
    },
    {
      "path": "LICENSE",
      "sha256": "e570a435f7675aa03ba17ee4dadba30c257308aef0bebb67e3789f3bcc100cd7"
    },
    {
      "path": "build.rs",
      "sha256": "626ee1d26b7774d02865e0c68c72745909b53a1a69602ff87374214b4cc9eb8b"
    },
    {
      "path": "docs/windows/windows-port-plan.md",
      "sha256": "204073460c96a1c08c786759a4ea571a795e00d16fa9d5ac3e45916e02151951"
    },
    {
      "path": "docs/windows/windows-test-plan.md",
      "sha256": "bc21647e96dd1e3fcab8c550f5da070fbc1258e71ecccc29030bd617de9dfde7"
    },
    {
      "path": "packaging/windows/README-WINDOWS.txt",
      "sha256": "d7ca64311c4842829cc67cda7e960fb6da33e4cf6f725401ba8f381b72c92eeb"
    },
    {
      "path": "packaging/windows/beek.iss",
      "sha256": "7f1d5ab2fce7ae037dc3375aa2507e5c00884ec78fc1cf43e35795530cca3cec"
    },
    {
      "path": "packaging/windows/installed.marker",
      "sha256": "7d0698689b2d55cbce578d325da39bae00d260dc71c14a26909461903cc06ca6"
    },
    {
      "path": "resources/BeeK.entitlements",
      "sha256": "8d90106151fd497f4e422b1559e07e3bb913c2325bf73aa5f553a6a3985a4094"
    },
    {
      "path": "resources/GPUI-INPUT-LICENSE-APACHE",
      "sha256": "752daf2fb234ca4a1fa372c073fe127f44b7b90fd2529ae44273a64f9d53da7a"
    },
    {
      "path": "resources/GPUI-INPUT-NOTICE.md",
      "sha256": "240cf73f556ddf926618d29ee7e4a6b04f0e891292fdb584cb73848b37445a35"
    },
    {
      "path": "resources/Info.plist",
      "sha256": "7643127036f443d69588fb1526eab20cd3d3d49dcba111c168e0c7371f56c4d7"
    },
    {
      "path": "resources/icons/BeeK-app-256.png",
      "sha256": "7eb83060020ecdda79b86e5e5567931bff752a0fdd98a40e7addf3ea657e146f"
    },
    {
      "path": "resources/icons/BeeK.icns",
      "sha256": "f435d875f0eb24d965cfa6f0bc9a49672eccee5a8cf6ddcc48bdaeb41e853f5f"
    },
    {
      "path": "resources/icons/BeeKMenuBarTemplate-white@2x.png",
      "sha256": "dcee2242709faf18a39dd0844f5105186d915f824239713e9798e1b6976781e1"
    },
    {
      "path": "resources/icons/BeeKMenuBarTemplate.png",
      "sha256": "c47b35f883bfda21948ca688f304b3d0c719f1d6b5827958d588e77176dfc529"
    },
    {
      "path": "resources/icons/BeeKMenuBarTemplate@2x.png",
      "sha256": "b107133225cd952b89ab1f23a762117e4c59d303756a07940676a4210609a781"
    },
    {
      "path": "resources/icons/beek-circle-i-white@4x.png",
      "sha256": "ca8c94438b821fda459f3ffb540c3949063ba8b3af42e5dcbedf3ff967854ce9"
    },
    {
      "path": "resources/icons/beek-circle-i@4x.png",
      "sha256": "fe85045b3a667f7dd6d02cbd7f2e5c0fc298ab6bc27f3540579da01bb353c93e"
    },
    {
      "path": "resources/icons/beek-selection-default-32.png",
      "sha256": "15b97a5b6bc87009fe17808b2bef57edee897671ae60e1c8554c9c500f66b634"
    },
    {
      "path": "resources/icons/beek-selection-default-32@2x.png",
      "sha256": "f5f54bb3eb0fc1af92888d0a1c4e97970af57da0ee3f2f8127aa43a906083b6c"
    },
    {
      "path": "resources/icons/beek-selection-disabled-32.png",
      "sha256": "253ef46aa6522983399bafbbfaab5dc85192a77e59f2f7a52a5beef8b16a7c29"
    },
    {
      "path": "resources/icons/beek-selection-disabled-32@2x.png",
      "sha256": "28405571ba8865da0c6b3fa4689f91e24a0f7106f4817c48a4ac91c19395b9b0"
    },
    {
      "path": "resources/icons/beek-selection-hover-32.png",
      "sha256": "bca0b97e5d908becadba3e35bcb82103981a40c5641b5af53cacf8170184e41b"
    },
    {
      "path": "resources/icons/beek-selection-hover-32@2x.png",
      "sha256": "e3e1fe6295b32d9faaef55ac1e96428d9f640ee428bd5805f5656ad0e449576d"
    },
    {
      "path": "resources/icons/beek-selection-pressed-32.png",
      "sha256": "1b1f65cfecbe08ab0bd304291da9367de077732f08377cb001dd36c86a676eda"
    },
    {
      "path": "resources/icons/beek-selection-pressed-32@2x.png",
      "sha256": "9aefa0ff60f5eecc88068be745743a5c9abfe86008897d39ee0dcb7bff834d16"
    },
    {
      "path": "resources/icons/beek-speak-outline@4x.png",
      "sha256": "cbab2d3d5a17b35e427633515ebb67bcefc4afd1a31ab37abc2b47013d0e0419"
    },
    {
      "path": "resources/icons/gen_circle_i.py",
      "sha256": "8d909f5115c7d8120312cdb6b3e0b3b9285f78291d56a42dcf376830a4a951a9"
    },
    {
      "path": "resources/icons/gen_speak_outline.py",
      "sha256": "c514f1c774f3208b148cf90779a8e132e48c7e5e7a573fa7f990a679e8d391d1"
    },
    {
      "path": "resources/icons/gen_template_white.py",
      "sha256": "53cc56987322cd29ae7557b93bd400721bf725edf58b5a2abe8b6c364907eab2"
    },
    {
      "path": "resources/windows/BeeK.ico",
      "sha256": "69b54b73006b680cddaa175d9b0fb77c0424df16d925a6289417836ac17c764f"
    },
    {
      "path": "resources/windows/beek.rc",
      "sha256": "54fa0e1527817735028df6cf8d4351ef1769fced342847f471024e77cf8adfb3"
    },
    {
      "path": "resources/windows/licenses/blade-graphics-0.7.1.txt",
      "sha256": "1f86e4aff170b7648ed38c44f61bfed5fb15daae4162b6d9db3449461070c2ac"
    },
    {
      "path": "resources/windows/licenses/blade-macros-0.3.0.txt",
      "sha256": "87dcd63407d6fc7b319460d6116995a45e547d0e3f27ddb62d4325e51743ec82"
    },
    {
      "path": "resources/windows/licenses/blade-util-0.3.0.txt",
      "sha256": "bf1c62a4afe65ffe9a322b3cef24dcdf44ed39bea87deb968a7205cac28b186a"
    },
    {
      "path": "resources/windows/licenses/convert_case-0.4.0.txt",
      "sha256": "4f641189fac72b80c8085b14caa27dceb62d69b06ad2e3123273bedf554447cb"
    },
    {
      "path": "resources/windows/licenses/dtor-0.0.6.txt",
      "sha256": "275a3b22cf7f01093449673a5e1011c92983edd1df98e64688d65a08ad14a246"
    },
    {
      "path": "resources/windows/licenses/dtor-proc-macro-0.0.5.txt",
      "sha256": "647a542a69dcb5c744ae20ee79b1706a4706ffb8a9d28e564b73a2b65a8d5a01"
    },
    {
      "path": "resources/windows/licenses/gpu-alloc-0.6.2.txt",
      "sha256": "1a89c9c4207a2c84489b08d1615fb9c5fdcf18459996a37bcd28fc05548eabe7"
    },
    {
      "path": "resources/windows/licenses/gpu-alloc-ash-0.7.1.txt",
      "sha256": "56d7570a9aa67257f073b09566a770ac9b15d0b33f1dd5bbead2b9efb4fa2e81"
    },
    {
      "path": "resources/windows/licenses/gpu-alloc-types-0.3.1.txt",
      "sha256": "8a1c6cdf3c7abd729e3d93d7f57d3c0aca8b51f75e560b4b0cc21dec6fc9b37b"
    },
    {
      "path": "resources/windows/licenses/hexf-parse-0.2.1.txt",
      "sha256": "07f945cfb4130b8ba4e9b3945d6934616827d626805a42e8466a658a6e8dcb43"
    },
    {
      "path": "resources/windows/licenses/lyon-1.0.19.txt",
      "sha256": "55b3595a3096179e77edec298e91b5657ed76ae4d326796f6d883689a226398a"
    },
    {
      "path": "resources/windows/licenses/lyon_algorithms-1.0.21.txt",
      "sha256": "21e8e6bca21fa6d753940bddc949eadf38357ab180a022d229dea7f1af24b36b"
    },
    {
      "path": "resources/windows/licenses/lyon_geom-1.0.19.txt",
      "sha256": "e0884da9dff3243002151be411e5f7aabfc234ecdd162ece9544f0215f646f16"
    },
    {
      "path": "resources/windows/licenses/lyon_path-1.0.19.txt",
      "sha256": "1cdb689a9e1445c771057062bafb570241558adf0a82e49811652ff3d3d94905"
    },
    {
      "path": "resources/windows/licenses/lyon_tessellation-1.0.22.txt",
      "sha256": "8f1a873be2af0d62774fedcb9527e21836c605f4dac30c913faea3a1b980da50"
    },
    {
      "path": "resources/windows/licenses/mac-0.1.1.txt",
      "sha256": "c45301b13e6dc15249a3c62ee485ecdd7f19fd3d66da8e6e4e3c510f5afb9c4f"
    },
    {
      "path": "resources/windows/licenses/naga-25.0.1.txt",
      "sha256": "12b7c98a78f5739e38dc5ddba93ce09d8cc2aced299e7809e1a37593bd98b69d"
    },
    {
      "path": "resources/windows/licenses/profiling-1.0.18.txt",
      "sha256": "14750284a061d35e797f35dad550ffe469d43f3a80016b2138f757a4e670b467"
    },
    {
      "path": "resources/windows/licenses/profiling-procmacros-1.0.18.txt",
      "sha256": "c72855afc95101b7fe1cf6b5fb2eeaa3176bb683e5eeffc0b8a2e295fcaa93e6"
    },
    {
      "path": "resources/windows/licenses/pulp-wasm-simd-flag-0.1.1.txt",
      "sha256": "899312a7e6b00e6695e6441f2e3dd6d3d54d4598e02e9325e07c4f2b66724834"
    },
    {
      "path": "resources/windows/licenses/seahash-4.1.0.txt",
      "sha256": "04c75992ef5fcf9136d8d84054091bc42aaa55ccccf2e0239855b37578edb594"
    },
    {
      "path": "resources/windows/licenses/simd_helpers-0.1.0.txt",
      "sha256": "c08e2177bcc919a3e95f003305a67ccd96fb1fffd4cf1f18e2002bc74cfc4538"
    },
    {
      "path": "resources/windows/licenses/spirv-0.3.0+sdk-1.3.268.0.txt",
      "sha256": "2fa8139fea72ce1e69b968519b25cac26f660c538eecdf1b1758d956ea902f3d"
    },
    {
      "path": "resources/windows/licenses/stacksafe-0.1.4.txt",
      "sha256": "197623f37430dfb2cfc260d80d6f966469702d24372ee7691d623038fbf18c92"
    },
    {
      "path": "resources/windows/licenses/stacksafe-macro-0.1.4.txt",
      "sha256": "108c083b14ee2613c4b90c4cc02ceaac25d53f34174d1da1f814ab191f4fd3d1"
    },
    {
      "path": "resources/windows/licenses/svg_fmt-0.4.5.txt",
      "sha256": "a06ffb99e39f332d15e5c89a695906cc0a740a41a2936f2dc57d5410988ec1c8"
    },
    {
      "path": "resources/windows/licenses/taffy-0.9.0.txt",
      "sha256": "7cd3b064a63f8cbf03a6129281c34be643fb7fc0854f3de1928c9c64a986209a"
    },
    {
      "path": "resources/windows/licenses/zune-inflate-0.2.54.txt",
      "sha256": "9d29d5a15d40fa5b7dc6f08b94b5a423efde9ee42aedf0eb0323a27da3db45ee"
    },
    {
      "path": "rust-toolchain.toml",
      "sha256": "ca67e66659a82661386e5a08471c913cb8b3f7c7ce37592207d3f513589a41c2"
    },
    {
      "path": "scripts/build_bundle.sh",
      "sha256": "598e5f39152e2945757ffffe582c30a93a0fcc002dc368f18ee186be79a4eabc"
    },
    {
      "path": "scripts/dev/check_shell_vars.py",
      "sha256": "b7bdd9509b237e6934485c1eef1308fd58bb6aacce5f0591cba789ccb17279f8"
    },
    {
      "path": "scripts/dev/make_developer_id_cert.py",
      "sha256": "409d125961f96079e8c5d9b90c96a1538c7dfd7bebe2ae4d9278b505bc913722"
    },
    {
      "path": "scripts/package_app.sh",
      "sha256": "4d8d55f42ac9024b4c8cf9b2898fd7a9b41959b183eb13cda090363f4a55d1be"
    },
    {
      "path": "scripts/release.sh",
      "sha256": "3e1286fdf9e0a5641230df4c02358631c0e4d9f464022ba98bf4eba687ef439b"
    },
    {
      "path": "scripts/windows/collect-environment.ps1",
      "sha256": "fc07feea1d88aab603aa50de74d6b5ecf8cd31288da4a37a62cd91571a11e7fa"
    },
    {
      "path": "scripts/windows/desktop-agent.ps1",
      "sha256": "dfa1dbcb8c71c867fb1c1d3fd774a55304e048618692a188887d1515a3b36078"
    },
    {
      "path": "scripts/windows/desktop_client.py",
      "sha256": "29368f26ede194816fc1296c31bc8b23ba181eb38add4ee4c827e38422959633"
    },
    {
      "path": "scripts/windows/evidence.py",
      "sha256": "fce7ef97802a437bf510830d882259e7d3bf549c26b7ca2594296dab2a9a940e"
    },
    {
      "path": "scripts/windows/licenses.py",
      "sha256": "cdaf0763bb36a9db364a314a5e05e4490d52979bab7721d262fd46923d3fbce1"
    },
    {
      "path": "scripts/windows/measure-idle.ps1",
      "sha256": "e416df1ee398783e40ad9bd6ed4aeb25296a3ab9ca8b07a0e45783dde9a0ff37"
    },
    {
      "path": "scripts/windows/metrics.py",
      "sha256": "84df50e928e36c0127b579e75107b2b7476e8aeb6391f7549277510d447bbe76"
    },
    {
      "path": "scripts/windows/mock_server.py",
      "sha256": "87dc2f7a1eaa55a431e928d06e32cce8a808178fd993c5fb77df8d617295a39e"
    },
    {
      "path": "scripts/windows/package.ps1",
      "sha256": "9ed5d4e57dab0cd046fa024638dc422e46a394eaca58f275575818abe7907e5a"
    },
    {
      "path": "scripts/windows/release_assets.py",
      "sha256": "8b1df47736d117e8edcc332220aa80d1d365e7843546c4785ea1495a303fc28a"
    },
    {
      "path": "scripts/windows/run-g0.ps1",
      "sha256": "1c714554cc551a07cf8514f2c3770b0d75181c1589874ba613e3ef52845a83fb"
    },
    {
      "path": "scripts/windows/run-product.ps1",
      "sha256": "6c8eaffc5be149d733739db39c47bf048461164214dab32c83f93975c15efe2c"
    },
    {
      "path": "scripts/windows/trace_check.py",
      "sha256": "4c4e61d35f5ef8baf26b652164f727522aee7f8fc7a0159a5fc4b1194437df0c"
    },
    {
      "path": "src/appcopy.rs",
      "sha256": "04ec326c1fc8c93ecdcf1d9c251ef8b3c44c9fcf5edf5715cd13197e8b4dba5c"
    },
    {
      "path": "src/ax.rs",
      "sha256": "7fe1b91f4496751b3e4605ab2c0eb919c1dc8af3a25ab329344d2973fa086dbf"
    },
    {
      "path": "src/bin/selection-host.rs",
      "sha256": "55686596322bc3253f099360f0d681fa3a04bebff3d5caad770156000755c339"
    },
    {
      "path": "src/bin/windows-harness.rs",
      "sha256": "753550fa12cdd5b20f1faf633671ebd3f9b4eafeac9b8784434a94df96ad01b5"
    },
    {
      "path": "src/clipboard.rs",
      "sha256": "c588fcee834b8331d7761b4ebc44160edb0ad2b2d0f842f98b05eec40e813f8d"
    },
    {
      "path": "src/config.rs",
      "sha256": "7d1471754024a054095999afb70ad0beed607548e432ed75c491c2d88723412f"
    },
    {
      "path": "src/core/mod.rs",
      "sha256": "9470993940b810bc62cda2c5f705bb2410fdcf20e4d8a6c6e45e3e271ee195ce"
    },
    {
      "path": "src/core/policy.rs",
      "sha256": "b2d6d721e0a18a0235859527d9947fe8cbcf090ab5bb77d9440e5fd165a6fb93"
    },
    {
      "path": "src/core/prompts.rs",
      "sha256": "5fa7eefd2fdff213acbd69672a1b6da7776f8872f06a83f2aa5deff6a174d72d"
    },
    {
      "path": "src/core/protocol.rs",
      "sha256": "ce60a9909f3277c7bc3936a77bec0f4fa21708ecc05096e31444c64b1e0ff1e0"
    },
    {
      "path": "src/core/release.rs",
      "sha256": "f67daa9f8147ef60abaf57161be3430c125c19200141ce27875cba273495308a"
    },
    {
      "path": "src/core/settings.rs",
      "sha256": "3aba0bb7e144f6f9f852976ff226d479bac1eefb4962dc79a0e7fe578bf42098"
    },
    {
      "path": "src/core/stream.rs",
      "sha256": "553b85fb623436b090970af4cc5b3d96f87491fec94f7edab7c3d79e58dfb9b9"
    },
    {
      "path": "src/dialogs.rs",
      "sha256": "0dba436c5d40848340c07816a3afb0abfcddc7b720a417dd02152995413f2ea8"
    },
    {
      "path": "src/eventtap.rs",
      "sha256": "5dc13ca1304d7ae5f3e3db42d1865ac369a57b63fcbc7107972b4539ae925c3e"
    },
    {
      "path": "src/icons.rs",
      "sha256": "a2f8b54c64ea303005a63ea6bd6103c62ebcb18e0d2c6c3c277023bde60d9aa4"
    },
    {
      "path": "src/lib.rs",
      "sha256": "ae61c415accfd8f59c7d3b898e94fb865b45e572dc9a50bff0dee039786fdef9"
    },
    {
      "path": "src/macos_main.rs",
      "sha256": "9a4c08c45e720da5af693df72d257f5fd30ad5e92c482996397c2a646e8c760d"
    },
    {
      "path": "src/main.rs",
      "sha256": "f4e9053eefb405845d1375b6dcb914b3f9c8b888bb474e807d0d557893da8e47"
    },
    {
      "path": "src/menubar.rs",
      "sha256": "2e0205231375c27496fce2629d4a43c9469f5202c990b075b40fe35ea97f4126"
    },
    {
      "path": "src/mouse.rs",
      "sha256": "f4bdc9b81e5f1024c251f5366fad7ab7de041b2d4838bbebb283b23bb2b3515e"
    },
    {
      "path": "src/platform/mod.rs",
      "sha256": "2efce5d11116c75559027c87bd17d4f6d5079ccbef0859f0becffbeabd2dc410"
    },
    {
      "path": "src/platform/windows/diagnostics.rs",
      "sha256": "7d833095fb19aec7b351a94ad47b936d9ada86fbf9d40a221b6ac2ab19eb8f56"
    },
    {
      "path": "src/platform/windows/harness.rs",
      "sha256": "6ec1a05c7e15265f71e8832bf496fb400a485e24e8f658ce9d21116d80a6ada2"
    },
    {
      "path": "src/platform/windows/helper.rs",
      "sha256": "f27be14dfc137a28f4124ce6ef919a4b8455d0727b69ccaecc35317bd17dc3a4"
    },
    {
      "path": "src/platform/windows/input.rs",
      "sha256": "f4c6d8190ab17d7e933f89471bce64a23dc046a5ab6c9a24cbef9fc8991f220a"
    },
    {
      "path": "src/platform/windows/integration.rs",
      "sha256": "226eb1ffa50dfdf825a05d67923730b4fcd9524b34ae438db02661957f64bb0a"
    },
    {
      "path": "src/platform/windows/lifecycle.rs",
      "sha256": "63dea1fa77e121af874a05d478515fdbc90a8d6df0e0091af566bfd053cdf39e"
    },
    {
      "path": "src/platform/windows/mod.rs",
      "sha256": "5ca2350bf077b18fabf4647dd00885cc4ab677095deb0643695f722a350432ba"
    },
    {
      "path": "src/platform/windows/native.rs",
      "sha256": "f038dc4be86261b1289e1520f5196545b91fb6d5f1f8d78d0f7ad94ce42a3ff5"
    },
    {
      "path": "src/platform/windows/pair.rs",
      "sha256": "3fd843890b32cd66f95318ed2b75a9741419f8b078e7d24139f24969e1f100b7"
    },
    {
      "path": "src/platform/windows/process.rs",
      "sha256": "37a312244895e7e932fa94a4f6714d14b39378b38077a1691f8e93be97a26ea9"
    },
    {
      "path": "src/platform/windows/selection.rs",
      "sha256": "1015e9bb635974454ee18a532b1b9367ba477d15861012314535b7d79570c24c"
    },
    {
      "path": "src/platform/windows/speech.rs",
      "sha256": "8effe98da5c4290aa4b5a712d2de534abd7b228cc192a44ba8d53cebf096cbc9"
    },
    {
      "path": "src/platform/windows/store.rs",
      "sha256": "ebc591ca619a1e18f34b8b47b54c4b84631393221b4d7c80e6cee7dad5d17c6f"
    },
    {
      "path": "src/platform/windows/text_model.rs",
      "sha256": "5b1aaca7d282a4c5a169d6d1d0f9e0b2e751ca9daeb2c062c79c35255e4e3c79"
    },
    {
      "path": "src/platform/windows/ui.rs",
      "sha256": "83997c66b80de341dd0462216040cd0d314c365671e322e57264526f8ba70240"
    },
    {
      "path": "src/platform/windows/util.rs",
      "sha256": "ca2993362c839de0e313335219388a8938eaaaeaa893ccc91e2f060c04e703fe"
    },
    {
      "path": "src/translate.rs",
      "sha256": "46d6c09bb64bf6606b86d0c9f569bdcf81d367314815bf4f739c3e8914457fb5"
    },
    {
      "path": "src/update.rs",
      "sha256": "42d5016af6518245f0c2acc00cd9c0690c62cfeea06fceab65c8115daa2139f4"
    },
    {
      "path": "tests/windows/cases.json",
      "sha256": "908c881d0d621877537e793a6c67e783d29d051a0f4548ee26c7df3943266582"
    },
    {
      "path": "tests/windows/fixtures/beek-g0-public-fixture.txt",
      "sha256": "a34b997bc2ecef23e5b6b01a816c8c0a597f70d3ca1404ec90dcb74f4e337611"
    },
    {
      "path": "tests/windows/fixtures/selection-dark.html",
      "sha256": "7fbfc3ebfddc69f09d7841de6279b4ab9bee2ce5b96316e37963302870ff4919"
    },
    {
      "path": "tests/windows/fixtures/selection.html",
      "sha256": "b57e3e0e07e5e9dd7a959dbb75090f4077bbfd5d4f3b7ab936870bd913dc0873"
    },
    {
      "path": "tests/windows/fixtures/selection.txt",
      "sha256": "32c297b35ffa78020fe2d30ebc6d10259036a4accb4b593dd36ea9c559d5f280"
    },
    {
      "path": "tests/windows/test_release_assets.py",
      "sha256": "12bf8a228b45fdb08989e2128c60fa3e8f7b67d642818070d37e0bee55bf0e36"
    },
    {
      "path": "tests/windows/test_tools.py",
      "sha256": "126c13ad2d8acb4dd429c92232667c7b43c4a62f3a6977d087081f828addcf09"
    }
  ]
}


## selection-review-3.md
复核完成（附件 330 行已完整读取）。**结论：维持有限 PASS**——selection 子系统（UIA/进程/协议/退避）无未关闭重大问题；R2.n1 经真实调用链核实为有意设计，撤回 Minor 定级。

**剪贴板写入尾部（实测修复）**：正确。CloseClipboard 后才读发布序号并回填 `own_after`，与 helper 读回定序逻辑自洽；返回前双读序号+属主校验，竞态写入判 SourceChanged，有界且保守。哨兵先行、弱断言标记 inconclusive 的测试处理合理。

**R2.n1（INFINITE 等待）**：我 R2 的担忧源于当时依赖未附（假设其 Drop 落在 UI/退出路径）。实际链路核实：`RestartChild` 仅存在于 `restart()` 派生的后台监督/创建线程局部，未存入 `self`；`Instance::drop` 不 Drop 该对象、不 join 这些线程，仅 cancel+kill 候选作业，主退出等待有界（1100ms 监听线程+互斥释放）。内核级卡死的候选会永久占用唯一的 Busy 槽，禁止无界重试候选——错误文案已明示此意图。创建线程 5 秒超时兜底。判定：**有界的资源设计，n1 按 CLOSED（设计性）处理，非缺陷**。

**R2.n2**：已由 `Jobs::limited(1, None)` 单次设置解决，内存上限仅对 helper 作业生效，无双重覆写。CLOSED。

**新附依赖（完整性级别）**：`token()` 新增 TOKEN_MANDATORY_LABEL 解析，尺寸上限、usize 对齐、`count==0` 守卫、末位 sub-authority 取值均正确；`eligible` 增加 `other.integrity <= own.integrity` 与 UIPI 方向一致（非提升自身为 Medium，Low/AppContainer 目标不受影响），无误杀常规应用，缓存键与容量不变。未发现可触发缺陷。

**边界说明（非本子系统缺陷）**：`restart()` 的 `restart_events` 置 Some 后附件内未见失败路径的清理点，后续 `restart()` 是否被首条件永久拒绝取决于未附的事件消费方，请在 lifecycle 侧评审确认；剪贴板实测结论采信附件自述，非本次独立复测。

**范围与限制**：本 PASS 覆盖三轮快照内 selection 子系统源码正确性；G5 仍未通过，签名/多屏/干净机/8 小时证据仍缺，不构成可公开发布判断。

## storage-lifecycle-review-2.md
Session: sess_72889fb4-eecd-4f72-877a-333c3b8d7e03

# 复审结论：本子系统**有限 PASS**（G5 未通过，不可公开发布）

首轮五个问题全部核销，修复自查发现两个 Minor 新问题（不阻塞）。逐项依据如下。

## 首轮问题核实

- **Storage.B1 → CLOSED**：`transact` 入口把“无 ref 且 api_key 非空”的 Keep 升级为 Replace，复用既有 create→复核→commit→失败删 staged 保原 key 序列；`startup_settings` 启动即迁移落 ref，`Store::key` 不再出现“有明文却取不到”的常态。回归测试覆盖 commit 失败保明文/成功迁移/序列化不含明文。遗留边缘见 N1。
- **M1 → CLOSED**：`child_preflight` 等 `ParentExitCommitted` 改 `read_for(6000)`；父侧 grant 窗 5s、父读 ChildReady 的 δ<1s，6s > δ+5s 成立。公共 IPC 每操作仍 1000ms，未误伤。
- **M2 → CLOSED**：validate 补 base_url≤4096、hotkey≤64、credential_ref 非空≤128、api_key≤2560，叠加 model≤1024——任何可提交配置远低于 load 的 1MiB 拒绝线，“写出不可再读且应用内无法修复”的不变式恢复。
- **m1 → CLOSED**：备份名改内容散列（64hex）去重，同内容反复启动零拷贝；prune 保留最新 3 份、其余满 24h 删除，且兼容旧 48hex 命名。
- **m2 → CLOSED**：连续 50 次建管失败发诊断事件 + `Event::IpcUnavailable` + 降频 1s；成功即复位，再故障可再次触发。

## 修复自查新发现（Minor）

- **N1**：legacy api_key 为纯空白（或超 2560）时，迁移路径在 trim 后判空/超长报错——启动路径整份配置落到默认值+错误横幅；会话内 Keep 保存被拒。可恢复（重填或 Clear）。建议 `startup_settings` 对 trim 后为空的 api_key 直接返回 cfg 不迁移。
- **N2**：ChildReady 现排在 GPU 初始化与多次配置读取之后，父侧预算仅 connect(5s)+read(1s)；冷启动（杀软扫描/着色器编译）超约 6s 会使重启假性失败（旧实例续跑、可重试）。属有意换取“子进程验证可用后才提交”，建议放宽父侧 connect 预算。

## 新增交接逻辑核查（未发现问题）

Job 原子归属 + KILL_ON_JOB_CLOSE 防父崩溃孤儿；挂起态取消走 Drop 的 job.kill+等待，无挂起孤儿残留；`commit()` 清 KILL_ON_JOB_CLOSE 仅发生在收到 ChildCommitAccepted 之后，此后父崩溃的幸存者恰是已获授权的候选；用户 Quit（未消费 Committed）用显式 TerminateJobObject 杀候选，不依赖该标志；Busy 租约由重启线程+创建线程双持有，创建返回前不可重试；子进程 `bInheritHandles=false`，不污染会话锁；子进程确认父死亡后重读已提交配置，预检快照不一致已消除。

**未附依赖（存疑记录，非本轮问题）**：`Jobs` 包装的 kill/句柄语义、`diagnostics::event`、`Event::IpcUnavailable` 的 UI 处理、Root 对 `restart_events` 的复位。

此 PASS 仅限 storage-lifecycle 子系统本轮修复面；G5 整体未通过，不构成公开发布判断。


## network-packaging-review-3.md
# 复核结论：有限 PASS（仅限本子系统；G5 仍未通过）

## m6 关闭核验（调用点证据成立）

- `run()` 首条语句即分发 `--purge-user-data`，清理后立即 `return`（失败 `exit(1)`），先于会话检查、`ui::run` 及一切初始化；`install_guard()` 仅在 `ui::run` 内调用，purge 子进程永不取锁，与卸载器独占锁（含 m4 持锁至 `DeinitializeUninstall`）无交集。卸载链路 Exec→Code=0→继续删 Run 项，闭合。
- R2 的 purge 冲突判定确认为**因未附依赖而产生的假设，非真实代码缺陷**：实际调用点将参数分发置于 guard 之前，冲突在真实代码中不存在；卸载器无需提前放锁，m4 关闭状态不受影响。
- guard 语义在真实路径成立：正常启动经 `check_platform`→`install_guard()?`，Err 经 `util::fatal` 退出（安装/卸载期间启动即拒并提示），`Ok(None)` 兼容无 marker 的便携运行。

## R2 遗留 Minor 关闭

`let _installation = install_guard()?` 为具名绑定而非 `_`，按 Rust 语义存活至 `ui::run` 栈帧结束（即 `Application::run` 返回），句柄全程持有，“调用方提前 drop 放锁”的担忧消除。

## 附带核验（未另开支线）

- `--capabilities`/`--verify-pair` 同样在会话检查前分发，CI（session 0）可执行预检；`capabilities` 的 `native_machine`、`test_fixtures` 字段与 package.ps1 的 5 秒预检、34404 原生门禁、fixture 拒绝逻辑逐项对应闭环。
- 会话检查要求 `session != 0`，与交互式桌面前提一致，且不影响已分发的运维子命令。
- 应用运行中手动直跑 `--purge-user-data` 不经 guard，属用户显式破坏性命令，不计为本子系统缺陷。

## 新增实测修复

本附件无新增差异或实测材料，仅调用点证据，无复核对象；如实说明，不作臆测。

## 新引入 Bug

证据代码内未发现可触发问题（分发顺序、错误退出码、绑定生命周期均核对）。

## 裁定

七项问题全部 CLOSED，无未关闭重大问题。对该子系统（附件所及网络池/SSE、更新检查、资产校验、打包与安装器范围）给予**有限 PASS**。保留项不变：未签名预览、缺外部实测仍为发布阻塞，G5 未通过，不可公开发布。

## ui-lifecycle-review-4.md
# R4 增量复审（最终意见）

**结论：本增量有限 PASS。** 新增实现未发现 Blocker/Major 缺陷；此前跨子系统留观项（restart 子进程静默退出）经 lifecycle 实码核实正式关闭；遗留 1 个 Minor（同缺陷类未覆盖到 keep_accessible 路径）与若干未附依赖假设，逐项列后。G5 仍未通过，不构成公开发布批准。

## 留观项关闭核实（lifecycle.rs 实码）

父侧 restart 线程对每条子进程失败路径都有界：创建等待 5s、管道连接 30s、提交回读 1s；子侧 `child_preflight` 提交等待 6s、父进程退出等待 10s。子进程在任何阶段死亡，父侧要么读管道失败要么超时，最终必达 `RestartEvent::Failed` → UI 置 `restart_events=None` 并在 form 显示错误，旧实例继续运行——"失败子不弹独立 fatal"因此安全，无僵尸等待。`commit_exit` 仅在 Committed 后调用，使 `Instance::drop` 不击杀已接管子进程；未提交则 cancel+job.kill 防孤儿。另核实崩溃恢复：进程死亡即关闭全部句柄、命名互斥体随之销毁，`CreateMutexW` 对已存在互斥体忽略 bInitialOwner，故崩溃后重开不会永久命中"已在运行"分支（该分支仅在前实例真实存活时进入）。`Event::IpcUnavailable` 也有了真实生产者（listener 连续 50 次建管道失败），R2 的死代码疑问一并消除。

## 新增代码核查

**1. WM_APP+23 延迟尺寸重投——设计正确，存在一处未覆盖。** 消息经 PostMessage 在 UI 线程借用结束后处理，规避 GPUI Window 双重借用；handler 内 `GetClientRect` 取当下物理尺寸（DPI 变化自校正），`WM_SIZE` 的 LOWORD/HIWORD 打包正确、clamp 防越界，经 `DefSubclassProc` 转交原窗口过程无指针生命周期问题，WM_NCDESTROY 反注册、消息号与 TRAY/CONFIG 无冲突、重复 show_popup 重投幂等。实测按钮可见，采信。
**缺口（Minor）**：`open_form`/快照 `open_panel` 的 `keep_accessible` 同样在 `w.update()` 借用内做含尺寸的 SetWindowPos（工作区小于窗口时收缩，如 1366x768 笔记本对 720x780 逻辑的设置窗），却不在这两个窗口装子类、也不投递同步消息——同缺陷类未覆盖。设置窗有 `overflow_y_scroll` 兜底可滚到保存键，快照 420 逻辑高度极少超工作区，故降为 Minor。最小修复：为这两类窗口复用同一子类+WM_APP+23，或 keep_accessible 仅钳位不缩尺寸。建议补一次实机验证借用失败是否在该路径复现。

**2. speech.rs——通过。** 固定 STA 线程、首次请求才 CoInitializeEx（避免消息泵前 COM 阻塞）、Speak 全程在该线程；输入通道容量 1、结果通道容量 2 均有界，`try_send` 失败即"朗读繁忙"，UI 每tick `poll` 且 `g==generation` 才应用——过时回执安全丢弃；不 join 线程，退出由 `exit_deadline` 兜底。仅一 cosmetic：结果通道满 2 时第三条回执丢弃，状态停在"正在朗读…"，不修可接受。

**3. 实时卡前台持续核对——通过，附带效应与设计一致。** 每 tick 校验 `!snapshot && !is_current(source)` 即取消网络并隐藏，补上 SKIPOWNPROCESS 漏掉自家 Settings 激活的缺口；快照豁免、拖拽/按钮不夺焦故不误杀。注意托盘右键菜单（`SetForegroundWindow` 到隐藏 owner）也会按同一规则关闭实时卡——属"实时卡跟随前台"设计内行为，记录非缺陷。

**4. 钳制与 phase 日志——通过。** `startup_settings` 顺带把纯空白 key 归零；`last_settings_status` 以 initial_error 种子化，--autostart 场景首次打开设置也能看到启动错误；phase 日志仅结构性字段，无配置/正文泄漏。

## 假设清单（未附依赖，未据此定性缺陷）

`process::RestartChild/Jobs`（commit/kill 语义）、`util::Handle` 关闭即释放、`util::is_current/work_area/current`、GPUI 对合成 WM_SIZE 的处理（已有实测佐证）。这些契约若变化需回溯上述结论。

## 最终判定

四项增量 + 留观关闭全部核实，仅余 1 个 Minor（keep_accessible 缩尺寸路径的延迟同步缺失，建议下批随实测一并修复）。源码层面**有限 PASS**；环境验收（多显示器、浸泡、签名）仍属 G5 范畴未完成，**不得公开发布**。

## native-review-5.md
# R5 复审（R4 唯一 Minor 核销）

**结论：Minor CLOSED，子系统维持有限 PASS，本轮未发现新引入缺陷。** G5 环境验收仍未通过，不得公开发布。

## 核实过程

**1. 尺寸同步覆盖 keep_accessible——成立。** `keep_accessible` 现在在与 R4 弹窗路径相同的次序下工作：装子类 → SetWindowPos（含尺寸钳制）→ PostMessage(WM_APP+23)；回调在 UI 线段借用结束后读取当下 GetClientRect 并以正确 LOWORD/HIWORD 打包转交原窗口过程。R4 缺口（设置窗/快照在 `w.update()` 借用内缩尺寸导致 GPUI viewport 陈旧）由此闭合。

**2. 普通窗口激活不受损——成立。** 子类 userdata 区分两类窗口：`window_adapter` 仅在 `no_activate != 0` 时拦截 WM_MOUSEACTIVATE。`show_popup` 先写入 WS_EX_NOACTIVATE 再以 refdata=1 装子类（与 R4 行为等价，无回归）；`keep_accessible` 按窗口当前实styles计算 refdata——设置/快照窗口为 0，鼠标激活走 DefSubclassProc 原链路，正常激活语义保留。不存在“弹窗拿到 refdata=0”的路径（样式在任何子类安装前已写入，且从不清除）。

**3. 最小化防护——成立。** WM_APP+23 处理前先 `IsIconic` 跳过，避免给最小化窗口合成 SIZE_RESTORED 造成错误 viewport；恢复时系统会投递真实 WM_SIZE 自行同步。GetClientRect 失败仍安全返回 0，下次 post 自愈。

**4. 边界复查无新缺陷。** 两处均用子类 id=1，同窗口重复安装是替换而非堆叠；同一 id 下 refdata 以最后一次安装为准，且两类安装来源对同一窗口计算的 refdata 一致。系统 epoch 周期性调 keep_accessible 会重复 post 合成 WM_SIZE，尺寸未变时幂等无副作用。`move_popup` 带 SWP_NOSIZE、`activate` 的 SW_RESTORE 不产生净尺寸变化，均无同类借用风险。PostMessageW 失败经 `?` 传播后被调用方忽略，窗口仍已完成定位，仅缺一次同步，属可接受降级。

## 遗留状态

- 源码层面：R1–R5 全部问题关闭，无未关闭的源码缺陷；假设清单（`process::RestartChild/Jobs`、`util::Handle`、`util::is_current`、GPUI 合成 WM_SIZE 处理——后者已有实机佐证并本轮机制一致）维持 R4 记录不变。
- 环境层面：多显示器、浸泡、签名等仍属 G5 范畴未完成，本有限 PASS 仅覆盖附加源码，**不批准公开发布**。

## final-codex-v1.md
# Codex 实现自审 v1

判断：源码范围有限 PASS；完整 G0–G5 未通过，不批准正式发布。

本轮基于四组首次异议、R2/R3修订、真实Windows失败记录与增量R4/R5复核。已核查来源级退避/取消恢复探针、终止进程计数、剪贴板最终发布序号、快速编辑重绑、配置跨存储事务、两阶段重启失败/退出边界、真实HTTP槽位与SSE限额、不可变安装目录及卸载锁、默认值与隐私日志。没有将外审假设当成事实；提供缺失调用点后，ZCode撤回了后台候选无限等待、卸载purge锁冲突、父侧PID清理的假设。

实测额外修复：CloseClipboard推进序号造成紧接剪切/粘贴误失败；GPUI借用内原生缩放造成视口陈旧、卡片按钮不可见；SAPI在消息循环开始前激活造成启动停滞。修复后重跑保留原始失败，弱断言明确inconclusive。普通窗口的同类尺寸同步缺口在R5补齐，正常激活和最小化语义保留。

本次最终清单：`final-source-manifest-v1.json`，SHA256 `584eb1f15f6b22408efd4212cec9cf2bb327b958ba0f925aee1bb83d25faadb4`；源码指纹 `6110fb9473824296a8173648e9fc7eb743a5b70900beb6a2548483a3c4b30f68`。清单由Codex本地重算；不声称ZCode独立重算。

代码检查、Windows23项真实交互测试、macOS核心19项和工具22项提供有限实证；当前最终release、安装/卸载及空闲回归仍以实际最新记录为准。签名、多屏/热插拔、干净机、第二GPU、8小时、完整L1/故障矩阵与macOS完整运行回归缺项均阻止正式发布。G6自动模拟复制与G7自动安装更新未启用。未提交、推送或发布。


## 最后测试入口（不进入正式产品）
```powershell
param([string]$OutputDirectory='', [switch]$BuildOnly)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if((Get-Process -Id $PID).SessionId -eq 0){throw 'Use a logged-in ordinary Windows desktop; SSH/service session is not acceptance.'}
if(-not $OutputDirectory){$OutputDirectory=Join-Path $repo ('test-results\product-'+(Get-Date -Format 'yyyyMMdd-HHmmss-fff'))}
if(Test-Path $OutputDirectory){throw 'Use a new evidence directory'}
$null=New-Item -ItemType Directory $OutputDirectory;$OutputDirectory=(Resolve-Path $OutputDirectory).Path
$logs=Join-Path $OutputDirectory 'logs';$null=New-Item -ItemType Directory $logs
function Invoke-Cargo([string[]]$Arguments,[string]$Name){$saved=$ErrorActionPreference;$ErrorActionPreference='Continue';& cargo.exe @Arguments *> (Join-Path $logs $Name);$code=$LASTEXITCODE;$ErrorActionPreference=$saved;if($code -ne 0){throw "cargo $Name failed ($code)"}}
function JsonCommand([string]$File,[string]$Arguments){$p=[Diagnostics.Process]::new();try{$p.StartInfo.FileName=$File;$p.StartInfo.Arguments=$Arguments;$p.StartInfo.UseShellExecute=$false;$p.StartInfo.RedirectStandardOutput=$true;$p.StartInfo.CreateNoWindow=$true;$null=$p.Start();$reading=$p.StandardOutput.ReadToEndAsync();if(-not $p.WaitForExit(5000)){$p.Kill();throw 'Binary preflight exceeded 5 seconds'};$text=$reading.GetAwaiter().GetResult();if($p.ExitCode -ne 0){throw 'Binary preflight failed'};$text|ConvertFrom-Json}finally{$p.Dispose()}}
Push-Location $repo
try{
 $envFile=Join-Path $OutputDirectory 'environment.json';& (Join-Path $PSScriptRoot 'collect-environment.ps1') -OutputFile $envFile
 Invoke-Cargo @('check','--locked','--all-targets') 'cargo-check.log'
 Invoke-Cargo @('test','--locked','--lib','--','--include-ignored','--test-threads=1') 'cargo-test-interactive.log'
 Invoke-Cargo @('build','--locked','--bins') 'cargo-debug.log'
 Invoke-Cargo @('build','--locked','--release','--bin','beek','--bin','beek-selection-host') 'cargo-release.log'
 $target=if($env:CARGO_TARGET_DIR){$env:CARGO_TARGET_DIR}else{Join-Path $repo 'target'}
 $main=Join-Path $target 'release\beek.exe';$helper=Join-Path $target 'release\beek-selection-host.exe'
 $caps=@{main=(JsonCommand $main '--capabilities');'selection-host'=(JsonCommand $helper '--capabilities')}
 foreach($kind in @('main','selection-host')){$file=if($kind -eq 'main'){$main}else{$helper};$caps[$kind]|Add-Member -NotePropertyName sha256 -NotePropertyValue (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()}
 $capsFile=Join-Path $OutputDirectory 'capabilities.json';[IO.File]::WriteAllText($capsFile,($caps|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
 Push-Location $env:TEMP
 try{$pair=JsonCommand $main '--verify-pair';if(-not $pair.paired){throw 'Helper pairing depends on cwd or failed'}}finally{Pop-Location}
 foreach($stage in @('G1','G2','G3','G4','G5')){
  $run=Join-Path $OutputDirectory ($stage+'\run.json')
  & python (Join-Path $PSScriptRoot 'evidence.py') create --stage $stage --out $run --environment $envFile --artifact $main --helper-artifact $helper --capabilities $capsFile
  if($LASTEXITCODE -ne 0){throw 'Evidence initialization failed'}
 }
 $g1=Join-Path $OutputDirectory 'G1';Copy-Item $logs (Join-Path $g1 'logs') -Recurse
 & python (Join-Path $PSScriptRoot 'evidence.py') record --run (Join-Path $g1 'run.json') --case G1-01-BUILD-WIN --status pass --actual 'Actual interactive Windows locked check, unit tests including real vault rollback, debug and release binary builds exited zero.' --evidence logs/cargo-check.log --evidence logs/cargo-test-interactive.log --evidence logs/cargo-debug.log --evidence logs/cargo-release.log
 if($LASTEXITCODE -ne 0){throw 'Build evidence recording failed'}
 if(-not $BuildOnly){Start-Process (Join-Path $g1 'artifacts\beek.exe')}
 Write-Host "Evidence initialized: $OutputDirectory. Only the Windows build case is recorded PASS; every remaining case needs actual evidence. Full G0 and G1-G5 remain gated."
}finally{Pop-Location}

```
