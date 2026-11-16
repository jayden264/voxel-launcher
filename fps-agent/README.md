# fps-agent（试验中，未完成）

给 Voxel 做“实时帧率检测”的小试验，**还没有编译、没有在真实游戏里验证过，不要当成可用功能**。

## 想法

Windows 不允许普通程序读取别的程序的帧率。所以这个 Java Agent 在游戏启动时通过 `-javaagent` 加载，
给 LWJGL 的 `GLFW.glfwSwapBuffers`（游戏每显示一帧调用一次）加一行“计数 +1”，再由后台线程每秒读一次计数，
写进输出文件（每行：`时间戳毫秒,帧数,最差100毫秒折算帧数`）。

- `GlfwPatcher.java`：直接改写 `GLFW.class` 的字节码，加一个 `voxelFrames` 静态字段并在方法开头自增。
  任何一处和预期不符就放弃（`patch` 返回 `null`），游戏照常运行。
- `VoxelAgent.java`：入口 `premain`、类转换器和每秒采样的后台线程。永远不向游戏抛异常。

## 还没做的

1. 用 Mojang 自带 Java 运行环境里的 `jdk.compiler` 编译（`--release 8`），打成 jar。
2. 用各版本真实的 `lwjgl-glfw.jar` 验证补丁能通过 JVM 字节码校验（`-Xverify:all`）。
3. 在 1.16.5（Java 8）、1.21.1（普通）、26.3 Fabric（Knot）、26.3 Forge（模块化）上真实启动，
   用 `maxFps:30/60` 设置对照读数是否准确，并确认不会让游戏崩溃。
4. 通过以上验证后，才开始做三个性能模式、低帧率弹窗和游戏结束后的帧率报告。

不支持 1.12 及更早的版本（它们用的是 LWJGL 2）。
