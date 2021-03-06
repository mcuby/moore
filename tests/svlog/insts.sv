///////////////////////
///  Empty modules  ///
///////////////////////

module A;
	B b();
endmodule

module B;
endmodule

//@ elab A
//| entity @B () () {
//| }
//|
//| entity @A () () {
//|     %b = inst @B () ()
//| }


//////////////////////////////////
///  Type parameter overrides  ///
//////////////////////////////////

module C;
	int t1;
	bit t2;
	D #(int) d1(t1);
	D #(bit) d2(t2);
endmodule

module D #(type T) (input T t);
endmodule

//@ elab C
//| entity @D.param1 (i32$ %t) () {
//| }
//|
//| entity @D.param2 (i1$ %t) () {
//| }
//|
//| entity @C () () {
//|     %t1 = sig i32
//|     %t2 = sig i1
//|     %d1 = inst @D.param1 (%t1) ()
//|     %d2 = inst @D.param2 (%t2) ()
//| }


//////////////////////////////////////
///  Default type parameter types  ///
//////////////////////////////////////

module E #(type T = bit) (input T t);
endmodule

//@ elab E
//| entity @E (i1$ %t) () {
//| }


//////////////////////////////////////////////
///  Dependencies between type parameters  ///
//////////////////////////////////////////////

module F;
	int x1;
	bit x2;
	G #(bit) g1(x2, x2);
	G #(int) g2(x1, x1);
	G #(int, bit) g3(x1, x2);
endmodule

module G #(type T, type R = T) (input T t, input R r);
endmodule

//@ elab F
//| entity @G.param3 (i1$ %t, i1$ %r) () {
//| }
//|
//| entity @G.param4 (i32$ %t, i32$ %r) () {
//| }
//|
//| entity @G.param5 (i32$ %t, i1$ %r) () {
//| }
//|
//| entity @F () () {
//|     %x1 = sig i32
//|     %x2 = sig i1
//|     %g1 = inst @G.param3 (%x2, %x2) ()
//|     %g2 = inst @G.param4 (%x1, %x1) ()
//|     %g3 = inst @G.param5 (%x1, %x2) ()
//| }


//////////////////////////
///  Value parameters  ///
//////////////////////////

module H;
	int x;
    I i1(x);
    I #(0) i2(x);
    I #(1) i3(x);
endmodule

module I #(int K = 0) (output int k = K);
endmodule

//@ elab H
//| entity @I () (i32$ %k) {
//|     drv %k 0
//| }
//|
//| entity @I.param6 () (i32$ %k) {
//|     drv %k 0
//| }
//|
//| entity @I.param7 () (i32$ %k) {
//|     drv %k 1
//| }
//|
//| entity @H () () {
//|     %x = sig i32
//|     %i1 = inst @I () (%x)
//|     %i2 = inst @I.param6 () (%x)
//|     %i3 = inst @I.param7 () (%x)
//| }


//////////////////////////
///  Port assignments  ///
//////////////////////////

module K (input int a, output int b);
endmodule

module L;
	int a, b;
	K k(a, b);
endmodule

//@ elab L
//| entity @K (i32$ %a) (i32$ %b) {
//|     drv %b 0
//| }
//|
//| entity @L () () {
//|     %a = sig i32
//|     %b = sig i32
//|     %k = inst @K (%a) (%b)
//| }

module M;
	int a, b;
	K k(a + 2, b);
endmodule

//@ elab M
//| entity @K (i32$ %a) (i32$ %b) {
//|     drv %b 0
//| }
//|
//| entity @M () () {
//|     %a = sig i32
//|     %b = sig i32
//|     %a0 = prb %a
//|     %0 = add i32 %a0 2
//|     %1 = sig i32
//|     drv %1 %0
//|     %k = inst @K (%1) (%b)
//| }

module N1;
	int a, b;
	K k(.a, .b);
endmodule

module N2;
	int a, b;
	K k(.*);
endmodule

module N3;
	int a, b;
	K k(a, .b());
endmodule
