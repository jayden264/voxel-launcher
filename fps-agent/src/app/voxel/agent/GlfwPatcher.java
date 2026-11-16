package app.voxel.agent;

import java.io.ByteArrayOutputStream;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;

/**
 * Adds one frame counter to LWJGL's {@code GLFW} class: a public static volatile long field,
 * incremented at the start of {@code glfwSwapBuffers(J)V} (which the game calls once per displayed frame).
 *
 * The counter lives inside GLFW itself on purpose. A call into a helper class would not link under
 * Forge's module layers, where game libraries cannot see the agent's class loader.
 *
 * Every assumption about the class file is checked; if anything is unexpected {@link #patch} returns
 * {@code null} and the game runs untouched. Written for Java 8 (no newer API), since old versions run on it.
 */
final class GlfwPatcher {
    static final String CLASS_NAME = "org/lwjgl/glfw/GLFW";
    static final String FIELD_NAME = "voxelFrames";
    private static final String METHOD_NAME = "glfwSwapBuffers";
    private static final String METHOD_DESC = "(J)V";
    /** getstatic(3) + lconst_1(1) + ladd(1) + putstatic(3); a multiple of 4 so tableswitch padding stays valid. */
    private static final int PREFIX_LEN = 8;

    /** Why the last {@link #patch} gave up, for the status line in the output file. */
    static volatile String lastRefusal = "";

    static byte[] patch(byte[] in) {
        try {
            return doPatch(in);
        } catch (Throwable t) {
            lastRefusal = "exception: " + t;
            return null;
        }
    }

    private static byte[] refuse(String why) {
        lastRefusal = why;
        return null;
    }

    private static int u1(ByteBuffer b) {
        return b.get() & 0xFF;
    }

    private static int u2(ByteBuffer b) {
        return b.getShort() & 0xFFFF;
    }

    /** Skip one attribute_info starting at the buffer's position. */
    private static void skipAttribute(ByteBuffer b) {
        b.position(b.position() + 2);
        int len = b.getInt();
        b.position(b.position() + len);
    }

    private static void w2(ByteArrayOutputStream o, int v) {
        o.write((v >> 8) & 0xFF);
        o.write(v & 0xFF);
    }

    private static void w4(ByteArrayOutputStream o, int v) {
        w2(o, (v >>> 16) & 0xFFFF);
        w2(o, v & 0xFFFF);
    }

    private static void copy(ByteArrayOutputStream o, byte[] src, int from, int to) {
        o.write(src, from, to - from);
    }

    private static int at2(byte[] a, int pos) {
        return ((a[pos] & 0xFF) << 8) | (a[pos + 1] & 0xFF);
    }

    private static byte[] doPatch(byte[] in) throws Exception {
        ByteBuffer b = ByteBuffer.wrap(in);
        if (b.getInt() != 0xCAFEBABE) return refuse("not a class file");
        b.getShort();
        b.getShort();
        int cpCount = u2(b);
        int cpStart = b.position();
        String[] utf = new String[cpCount];
        for (int i = 1; i < cpCount; i++) {
            int tag = u1(b);
            switch (tag) {
                case 1: {
                    int len = u2(b);
                    utf[i] = new String(in, b.position(), len, StandardCharsets.UTF_8);
                    b.position(b.position() + len);
                    break;
                }
                case 3: case 4: b.position(b.position() + 4); break;
                case 5: case 6: b.position(b.position() + 8); i++; break;
                case 7: case 8: case 16: case 19: case 20: b.position(b.position() + 2); break;
                case 9: case 10: case 11: case 12: case 17: case 18: b.position(b.position() + 4); break;
                case 15: b.position(b.position() + 3); break;
                default: return refuse("unknown constant pool tag " + tag);
            }
        }
        int cpEnd = b.position();
        b.position(cpEnd + 2); // access flags
        int thisIdx = u2(b);
        b.position(b.position() + 2); // super class
        int interfaces = u2(b);
        b.position(b.position() + interfaces * 2);

        // Fields: skip them, but make sure our field is not already there (patched twice).
        int fieldsCountPos = b.position();
        int fieldsCount = u2(b);
        for (int i = 0; i < fieldsCount; i++) {
            b.position(b.position() + 2); // access
            int name = u2(b);
            b.position(b.position() + 2); // descriptor
            if (FIELD_NAME.equals(utf[name])) return refuse("already patched");
            int attrs = u2(b);
            for (int a = 0; a < attrs; a++) skipAttribute(b);
        }
        int fieldsEnd = b.position();

        // Methods: find glfwSwapBuffers(J)V and its Code attribute.
        int methods = u2(b);
        int codeAttrStart = -1; // position of the attribute's name index
        int codeAttrEnd = -1;
        for (int i = 0; i < methods; i++) {
            b.position(b.position() + 2); // access
            int name = u2(b);
            int desc = u2(b);
            boolean target = METHOD_NAME.equals(utf[name]) && METHOD_DESC.equals(utf[desc]);
            int attrs = u2(b);
            for (int a = 0; a < attrs; a++) {
                int attrStart = b.position();
                int attrName = u2(b);
                int len = b.getInt();
                b.position(b.position() + len);
                if (target && "Code".equals(utf[attrName])) {
                    if (codeAttrStart >= 0) return refuse("two Code attributes");
                    codeAttrStart = attrStart;
                    codeAttrEnd = b.position();
                }
            }
        }
        if (codeAttrStart < 0) return refuse("no glfwSwapBuffers(J)V");

        // ---- parse the Code attribute
        ByteBuffer c = ByteBuffer.wrap(in);
        c.position(codeAttrStart + 2);
        int attrLen = c.getInt();
        int maxStackPos = c.position();
        int maxStack = u2(c);
        int maxLocals = u2(c);
        int codeLen = c.getInt();
        int codeStart = c.position();
        c.position(codeStart + codeLen);
        int excPos = c.position();
        int excCount = u2(c);
        c.position(c.position() + excCount * 8);
        int attrsPos = c.position();
        int innerCount = u2(c);
        for (int a = 0; a < innerCount; a++) {
            String n = utf[u2(c)];
            if ("StackMapTable".equals(n)) return refuse("method has a StackMapTable");
            c.position(c.position() - 2);
            skipAttribute(c);
        }
        if (c.position() != codeAttrEnd) return refuse("Code attribute length mismatch");
        if (attrLen != codeAttrEnd - codeAttrStart - 6) return refuse("Code attribute length mismatch");
        if (codeLen + PREFIX_LEN >= 65535) return refuse("code too long");

        // ---- write the patched class
        ByteArrayOutputStream o = new ByteArrayOutputStream(in.length + 128);
        w4(o, 0xCAFEBABE);
        copy(o, in, 4, 8); // minor, major
        int idxName = cpCount, idxDesc = cpCount + 1, idxNat = cpCount + 2, idxRef = cpCount + 3;
        w2(o, cpCount + 4);
        copy(o, in, cpStart, cpEnd);
        // Utf8 "voxelFrames", Utf8 "J", NameAndType, Fieldref(this class, NameAndType)
        o.write(1); w2(o, FIELD_NAME.length()); o.write(FIELD_NAME.getBytes(StandardCharsets.US_ASCII));
        o.write(1); w2(o, 1); o.write('J');
        o.write(12); w2(o, idxName); w2(o, idxDesc);
        o.write(9); w2(o, thisIdx); w2(o, idxNat);

        copy(o, in, cpEnd, fieldsCountPos);
        w2(o, fieldsCount + 1);
        copy(o, in, fieldsCountPos + 2, fieldsEnd);
        w2(o, 0x0001 | 0x0008 | 0x0040); // public static volatile
        w2(o, idxName);
        w2(o, idxDesc);
        w2(o, 0); // no attributes
        copy(o, in, fieldsEnd, codeAttrStart);

        // the Code attribute, with our four instructions in front
        w2(o, at2(in, codeAttrStart)); // attribute_name_index
        w4(o, attrLen + PREFIX_LEN);
        w2(o, Math.max(maxStack, 4)); // two longs on the stack
        w2(o, maxLocals);
        w4(o, codeLen + PREFIX_LEN);
        o.write(0xB2); w2(o, idxRef); // getstatic voxelFrames
        o.write(0x0A);                // lconst_1
        o.write(0x61);                // ladd
        o.write(0xB3); w2(o, idxRef); // putstatic voxelFrames
        copy(o, in, codeStart, codeStart + codeLen);
        // exception table: every pc moves by the prefix
        w2(o, excCount);
        for (int i = 0; i < excCount; i++) {
            int p = excPos + 2 + i * 8;
            w2(o, at2(in, p) + PREFIX_LEN);
            w2(o, at2(in, p + 2) + PREFIX_LEN);
            w2(o, at2(in, p + 4) + PREFIX_LEN);
            w2(o, at2(in, p + 6));
        }
        // nested attributes: tables of pcs move too
        w2(o, innerCount);
        int p = attrsPos + 2;
        for (int a = 0; a < innerCount; a++) {
            int nameIdx = at2(in, p);
            String n = utf[nameIdx];
            int len = ByteBuffer.wrap(in, p + 2, 4).getInt();
            int body = p + 6;
            w2(o, nameIdx);
            w4(o, len);
            if ("LineNumberTable".equals(n)) {
                int count = at2(in, body);
                w2(o, count);
                for (int i = 0; i < count; i++) {
                    w2(o, at2(in, body + 2 + i * 4) + PREFIX_LEN);
                    w2(o, at2(in, body + 4 + i * 4));
                }
            } else if ("LocalVariableTable".equals(n) || "LocalVariableTypeTable".equals(n)) {
                int count = at2(in, body);
                w2(o, count);
                for (int i = 0; i < count; i++) {
                    int e = body + 2 + i * 10;
                    w2(o, at2(in, e) + PREFIX_LEN);
                    copy(o, in, e + 2, e + 10);
                }
            } else {
                copy(o, in, body, body + len);
            }
            p = body + len;
        }

        // everything after the Code attribute (the methods count is unchanged: no method was added)
        copy(o, in, codeAttrEnd, in.length);
        lastRefusal = "";
        return o.toByteArray();
    }
}
