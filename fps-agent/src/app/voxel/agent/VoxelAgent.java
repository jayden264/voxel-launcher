package app.voxel.agent;

import java.io.BufferedWriter;
import java.io.File;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.security.ProtectionDomain;
import java.util.Collections;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;

/**
 * Voxel's frame counter. Loaded with {@code -javaagent:voxel-fps-agent.jar=<output file>}.
 *
 * It counts the frames the game displays (see {@link GlfwPatcher}) and once a second appends
 * {@code <unix ms>,<frames>,<worst 100 ms slice, as frames>} to the output file. It never throws into
 * the game: any problem just means no numbers, and the file says why on a {@code #} line.
 */
public final class VoxelAgent {
    private static volatile BufferedWriter out;
    private static volatile Instrumentation inst;

    private VoxelAgent() {}

    public static void premain(String args, Instrumentation instrumentation) {
        try {
            inst = instrumentation;
            if (args == null || args.trim().isEmpty()) return;
            File file = new File(args.trim());
            File dir = file.getAbsoluteFile().getParentFile();
            if (dir != null) dir.mkdirs();
            out = new BufferedWriter(new OutputStreamWriter(new FileOutputStream(file, false), "UTF-8"));
            note("voxel-fps 1");
            instrumentation.addTransformer(new Transformer());
            Thread t = new Thread(new Sampler(), "voxel-fps-sampler");
            t.setDaemon(true);
            t.start();
        } catch (Throwable ignored) {
            // The game must start no matter what.
        }
    }

    static synchronized void note(String line) {
        try {
            BufferedWriter w = out;
            if (w != null) {
                w.write(line);
                w.write('\n');
                w.flush();
            }
        } catch (Throwable ignored) {
            // nothing sensible to do
        }
    }

    private static final class Transformer implements ClassFileTransformer {
        @Override
        public byte[] transform(ClassLoader loader, String className, Class<?> redefined, ProtectionDomain domain, byte[] classfile) {
            if (!GlfwPatcher.CLASS_NAME.equals(className)) return null;
            byte[] patched = GlfwPatcher.patch(classfile);
            note(patched != null ? "#patched " + className : "#skipped " + GlfwPatcher.lastRefusal);
            return patched;
        }
    }

    /** Reads the counter and writes one line per second. */
    private static final class Sampler implements Runnable {
        private Field field;
        private boolean warnedAccess;

        private Field find() {
            Instrumentation i = inst;
            if (i == null) return null;
            for (Class<?> c : i.getAllLoadedClasses()) {
                if (!"org.lwjgl.glfw.GLFW".equals(c.getName())) continue;
                try {
                    Field f = c.getDeclaredField(GlfwPatcher.FIELD_NAME);
                    if (!canRead(f)) openPackage(c);
                    return f;
                } catch (NoSuchFieldException e) {
                    return null; // loaded but not patched (see the #skipped line)
                }
            }
            return null;
        }

        private boolean canRead(Field f) {
            try {
                f.getLong(null);
                return true;
            } catch (Throwable t) {
                return false;
            }
        }

        /** On Java 9+ the game's modules may not open their package to us; ask the JVM to. */
        private void openPackage(Class<?> glfw) {
            try {
                Method getModule = Class.class.getMethod("getModule");
                Object target = getModule.invoke(glfw);
                Object ours = getModule.invoke(VoxelAgent.class);
                Class<?> moduleClass = Class.forName("java.lang.Module");
                Method redefine = Instrumentation.class.getMethod("redefineModule", moduleClass, Set.class, Map.class, Map.class, Set.class, Map.class);
                Map<String, Set<?>> opens = new HashMap<String, Set<?>>();
                opens.put("org.lwjgl.glfw", Collections.singleton(ours));
                redefine.invoke(inst, target, Collections.emptySet(), opens, opens, Collections.emptySet(), Collections.emptyMap());
            } catch (Throwable t) {
                if (!warnedAccess) {
                    warnedAccess = true;
                    note("#cannot open module: " + t);
                }
            }
        }

        private long read() {
            try {
                return field.getLong(null);
            } catch (Throwable t) {
                if (!warnedAccess) {
                    warnedAccess = true;
                    note("#cannot read counter: " + t);
                }
                return -1;
            }
        }

        @Override
        public void run() {
            try {
                long lastTotal = -1;
                long secondStart = System.nanoTime();
                long secondFrames = 0;
                long worstSlice = Long.MAX_VALUE;
                int slices = 0;
                while (true) {
                    Thread.sleep(100);
                    if (field == null) {
                        field = find();
                        if (field == null) continue;
                    }
                    long total = read();
                    if (total < 0) continue;
                    if (lastTotal < 0) {
                        lastTotal = total;
                        secondStart = System.nanoTime();
                        continue;
                    }
                    long delta = total - lastTotal;
                    lastTotal = total;
                    secondFrames += delta;
                    worstSlice = Math.min(worstSlice, delta);
                    slices++;
                    long now = System.nanoTime();
                    if (now - secondStart >= 1_000_000_000L) {
                        // frames per second over the real elapsed time, and the worst 100 ms slice scaled up
                        double secs = (now - secondStart) / 1e9;
                        note(System.currentTimeMillis() + "," + Math.round(secondFrames / secs) + "," + Math.round(worstSlice * 10 * (slices > 0 ? 1.0 : 0)));
                        secondStart = now;
                        secondFrames = 0;
                        worstSlice = Long.MAX_VALUE;
                        slices = 0;
                    }
                }
            } catch (Throwable ignored) {
                // The sampler is the only thing that stops; the game does not.
            }
        }
    }
}
